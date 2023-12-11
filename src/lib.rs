// SPDX-FileCopyrightText: The rpds-pathtree authors
// SPDX-License-Identifier: MPL-2.0

//! Immutable, path-addressable tree data structure.

// Repetitions of module/type names occur frequently when using many
// modules for keeping the size of the source files handy. Often
// types have the same name as their parent module.
#![allow(clippy::module_name_repetitions)]
// Repeating the type name in `Default::default()` expressions is not needed
// as long as the context is obvious.
#![allow(clippy::default_trait_access)]
// TODO: Add missing docs
#![allow(clippy::missing_errors_doc)]

mod edge;
pub use self::edge::{HalfEdge, HalfEdgeRef, HalfEdgeTreeNodeRef};

mod node;
pub use self::node::{InnerNode, LeafNode, Node, NodeValue};

mod node_id;
pub use self::node_id::NodeId;

mod path;
pub use self::path::{PathSegment, PathSegmentRef, RootPath, SegmentedPath};

mod tree;
pub use self::tree::{
    AncestorTreeNodeIter, InsertOrUpdateNodeValueError, MatchNodePath, MatchedNodePath,
    ParentChildTreeNode, PathTree, PathTreeTypes, RemovedSubtree, ResolvedNodePath, TreeNode,
};

#[cfg(feature = "sync")]
type HashMap<K, V> = rpds::HashTrieMapSync<K, V>;

#[cfg(feature = "sync")]
fn new_hash_map<K: std::hash::Hash + Eq, V>() -> rpds::HashTrieMapSync<K, V> {
    rpds::HashTrieMapSync::new_sync()
}

#[cfg(not(feature = "sync"))]
type HashMap<K, V> = rpds::HashTrieMap<K, V>;

#[cfg(not(feature = "sync"))]
fn new_hash_map<K: std::hash::Hash + Eq, V>() -> rpds::HashTrieMap<K, V> {
    rpds::HashTrieMap::new()
}

#[cfg(test)]
mod tests;
