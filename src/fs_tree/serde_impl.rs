use std::{
    ffi::OsStr,
    fmt,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use log::warn;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, DeserializeSeed, Visitor},
    ser::SerializeMap,
};
use serde_repr::{Deserialize_repr, Serialize_repr};

use super::{
    DirNode, FileData, FileNode, FsTree, FsTreeConfig, FsTreeNode, FsTreeNodeId, NodeKind,
};

#[derive(Debug, Deserialize_repr, Serialize_repr)]
#[repr(i32)]
enum NodeTag {
    FILE,
    SYMLINK,
    ERROR,
}

const INVALID_UNICODE_MSG: &str = "was skipped during serialization: Path is not valid Unicode";

impl Serialize for FsTree {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        struct NodeRef<'a> {
            tree: &'a FsTree,
            id: FsTreeNodeId,
        }

        impl<'a> Serialize for NodeRef<'a> {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                let mut map = serializer.serialize_map(None)?;
                for child_id in self.id.children(&self.tree.arena) {
                    let child = self.tree.arena[child_id].get();

                    let Some(name) = child.name.to_str() else {
                        warn!(
                            "{:?} {INVALID_UNICODE_MSG}",
                            self.tree.get_full_path(child_id)
                        );
                        continue;
                    };

                    match &child.kind {
                        NodeKind::Dir(_) => {
                            map.serialize_entry(
                                &name,
                                &NodeRef {
                                    tree: &self.tree,
                                    id: child_id,
                                },
                            )?;
                        }
                        NodeKind::File(file_node) => {
                            let size = file_node.data.size;
                            let hash = file_node.data.hash;
                            let modified = file_node.modified.and_then(|m| {
                                m.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
                            });
                            map.serialize_entry(&name, &(NodeTag::FILE, size, hash, modified))?;
                        }
                        NodeKind::SymLink(data) => {
                            map.serialize_entry(&name, &(NodeTag::SYMLINK, data))?;
                        }
                        NodeKind::Error(err) => {
                            map.serialize_entry(&name, &(NodeTag::ERROR, err))?;
                        }
                    };
                }
                map.end()
            }
        }

        let mut map = serializer.serialize_map(None)?;
        for (node_id, root_path) in self.get_roots() {
            let path = root_path.join(self.get_node(node_id).name.clone());
            let Some(path) = path.to_str() else {
                warn!("{path:?} {INVALID_UNICODE_MSG}");
                continue;
            };
            map.serialize_entry(
                path,
                &NodeRef {
                    tree: self,
                    id: node_id,
                },
            )?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for FsTree {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FsTreeVisitor;

        impl<'de> Visitor<'de> for FsTreeVisitor {
            type Value = FsTree;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("fs tree")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                struct FsTreeCtx<'a> {
                    fs_tree: &'a mut FsTree,
                    name: &'a OsStr,
                }

                impl<'de, 'a> Visitor<'de> for FsTreeCtx<'a> {
                    type Value = FsTreeNodeId;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str("fs tree node")
                    }

                    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
                    where
                        A: de::MapAccess<'de>,
                    {
                        let node_id = self.fs_tree.arena.new_node(FsTreeNode {
                            name: self.name.into(),
                            kind: NodeKind::Dir(DirNode::default()),
                        });

                        while let Some(name) = map.next_key::<String>()? {
                            let child_id = map.next_value_seed(FsTreeCtx {
                                fs_tree: self.fs_tree,
                                name: OsStr::new(&name),
                            })?;
                            node_id.append(child_id, &mut self.fs_tree.arena);
                        }

                        Ok(node_id)
                    }

                    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                    where
                        A: de::SeqAccess<'de>,
                    {
                        let node_kind = match seq
                            .next_element::<NodeTag>()?
                            .ok_or_else(|| de::Error::missing_field("node tag"))?
                        {
                            NodeTag::FILE => {
                                let size = seq
                                    .next_element()?
                                    .ok_or_else(|| de::Error::missing_field("file size"))?;
                                let hash = seq
                                    .next_element()?
                                    .ok_or_else(|| de::Error::missing_field("file hash"))?;
                                let secs = seq
                                    .next_element()?
                                    .ok_or_else(|| de::Error::missing_field("file timestamp"))?;

                                let modified =
                                    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs));

                                let data = FileData { size, hash };
                                NodeKind::File(FileNode {
                                    data,
                                    modified,
                                    copies_count: 0,
                                })
                            }
                            NodeTag::SYMLINK => {
                                let text = seq
                                    .next_element()?
                                    .ok_or_else(|| de::Error::missing_field("symlink data"))?;
                                NodeKind::SymLink(text)
                            }
                            NodeTag::ERROR => {
                                let text = seq
                                    .next_element()?
                                    .ok_or_else(|| de::Error::missing_field("error text"))?;
                                NodeKind::Error(text)
                            }
                        };

                        let node_id = self.fs_tree.arena.new_node(FsTreeNode {
                            name: self.name.into(),
                            kind: node_kind,
                        });

                        if let NodeKind::File(file_node) = self.fs_tree.arena[node_id].get().kind {
                            self.fs_tree.index.fast_add(node_id, file_node.data);
                        }

                        Ok(node_id)
                    }
                }

                impl<'de, 'a> DeserializeSeed<'de> for FsTreeCtx<'a> {
                    type Value = FsTreeNodeId;

                    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
                    where
                        D: Deserializer<'de>,
                    {
                        deserializer.deserialize_any(FsTreeCtx {
                            fs_tree: self.fs_tree,
                            name: self.name,
                        })
                    }
                }

                let mut fs_tree = FsTree::new(FsTreeConfig::default());
                while let Some(key) = map.next_key::<String>()? {
                    let path = PathBuf::from(&key);

                    let (root_path, name) = match (path.parent(), path.file_name()) {
                        (Some(parent), Some(name)) => (parent, name),
                        (None, _) if path.has_root() => (Path::new(""), OsStr::new("/")),
                        _ => {
                            return Err(de::Error::invalid_value(de::Unexpected::Str(&key), &self));
                        }
                    };

                    let node_id = map.next_value_seed(FsTreeCtx {
                        fs_tree: &mut fs_tree,
                        name,
                    })?;
                    fs_tree.roots.insert(node_id, root_path.to_path_buf());
                }

                fs_tree.resolve_roots();

                Ok(fs_tree)
            }
        }
        deserializer.deserialize_map(FsTreeVisitor)
    }
}
