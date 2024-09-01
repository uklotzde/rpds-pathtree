// SPDX-FileCopyrightText: The rpds-pathtree authors
// SPDX-License-Identifier: MPL-2.0

//! Immutable, path-addressable tree data structure.

mod edge;
pub use self::edge::{HalfEdge, HalfEdgeRef, HalfEdgeTreeNodeRef};

mod node;
pub use self::node::{DepthFirstDescendantsIter, InnerNode, LeafNode, Node, NodeValue};

mod path;
pub use self::path::{PathSegment, PathSegmentRef, RootPath, SegmentedPath};

mod tree;
pub use self::tree::{
    AncestorTreeNodeIter, InsertOrUpdateNodeValueError, InsertedOrReplacedSubtree,
    InsertedOrUpdatedNode, MatchNodePath, MatchedNodePath, NewNodeId, PathTree, PathTreeTypes,
    RemovedSubtree, ResolvedNodePath, TreeNode, TreeNodeParentChildPathConflict,
    UpdateNodeValueError, UpdatedParentNode,
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
