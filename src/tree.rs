// SPDX-FileCopyrightText: The rpds-pathtree authors
// SPDX-License-Identifier: MPL-2.0

use std::{borrow::Borrow, fmt, hash::Hash, marker::PhantomData, num::NonZeroUsize, sync::Arc};

use thiserror::Error;

use crate::{
    new_hash_map, HalfEdge, HalfEdgeRef, HalfEdgeTreeNodeRef, HashMap, InnerNode, LeafNode, Node,
    NodeValue, PathSegment, PathSegmentRef, RootPath, SegmentedPath as _,
};

pub trait NewNodeId<T> {
    fn new_node_id(&mut self) -> T;
}

/// Type system for [`PathTree`].
pub trait PathTreeTypes: Clone + Default + fmt::Debug {
    type NodeId: Clone + Copy + PartialEq + Eq + Hash + fmt::Debug + fmt::Display;
    type NewNodeId: NewNodeId<Self::NodeId> + Clone + fmt::Debug;
    type InnerValue: Clone + fmt::Debug;
    type LeafValue: Clone + fmt::Debug;
    type PathSegment: PathSegment + Borrow<Self::PathSegmentRef>;
    type PathSegmentRef: PathSegmentRef<Self::PathSegment> + ?Sized;
    type RootPath: RootPath<Self::PathSegment, Self::PathSegmentRef>;
}

/// A conflicting path from a parent to a child node.
#[derive(Debug)]
pub struct TreeNodeParentChildPathConflict<T>
where
    T: PathTreeTypes,
{
    pub parent_node: Arc<TreeNode<T>>,
    pub child_path_segment: T::PathSegment,
}

#[derive(Debug, Error)]
pub enum InsertOrUpdateNodeValueError<T>
where
    T: PathTreeTypes,
{
    #[error("path conflict")]
    PathConflict {
        conflict: TreeNodeParentChildPathConflict<T>,
        value: NodeValue<T>,
    },
    #[error("value type mismatch")]
    ValueTypeMismatch { value: NodeValue<T> },
}

#[derive(Debug, Error)]
pub enum UpdateNodeValueError<T>
where
    T: PathTreeTypes,
{
    #[error("value type mismatch")]
    ValueTypeMismatch { value: NodeValue<T> },
}

impl<T> From<UpdateNodeValueError<T>> for InsertOrUpdateNodeValueError<T>
where
    T: PathTreeTypes,
{
    fn from(from: UpdateNodeValueError<T>) -> Self {
        let UpdateNodeValueError::ValueTypeMismatch { value } = from;
        Self::ValueTypeMismatch { value }
    }
}

/// Return type of mutating tree operations.
///
/// Updating an immutable node in the tree requires to update its parent node.
#[derive(Debug, Clone)]
pub struct ParentChildTreeNode<T>
where
    T: PathTreeTypes,
{
    pub parent_node: Option<Arc<TreeNode<T>>>,
    pub child_node: Arc<TreeNode<T>>,
    pub replaced_child_node: Option<Arc<TreeNode<T>>>,
}

/// Return type when removing a node from the tree.
#[derive(Debug, Clone)]
pub struct RemovedSubtree<T>
where
    T: PathTreeTypes,
{
    /// New parent node.
    ///
    /// Updated parent node of the removed node that remains in the tree.
    pub parent_node: Arc<TreeNode<T>>,

    /// Child path segment.
    ///
    /// Path segment between the parent node and its former child node,
    /// which has become the root node of the removed subtree.
    pub child_path_segment: T::PathSegment,

    /// Removed subtree.
    ///
    /// A new tree built from the removed node and all its descendants.
    pub removed_subtree: PathTree<T>,
}

impl<T> InsertOrUpdateNodeValueError<T>
where
    T: PathTreeTypes,
{
    pub fn into_value(self) -> NodeValue<T> {
        match self {
            Self::PathConflict { value, .. } | Self::ValueTypeMismatch { value } => value,
        }
    }
}

/// Cheaply clonable path tree structure.
///
/// Could be shared safely between multiple threads.
#[derive(Debug, Clone)]
pub struct PathTree<T>
where
    T: PathTreeTypes,
{
    root_node_id: T::NodeId,
    nodes: HashMap<T::NodeId, Arc<TreeNode<T>>>,
    new_node_id: T::NewNodeId,
    _types: PhantomData<T>,
}

#[derive(Debug, Default)]
struct TreeNodeParentChildContext<'a, T>
where
    T: PathTreeTypes,
{
    parent_node: Option<Arc<TreeNode<T>>>,
    child_path_segment: Option<&'a T::PathSegmentRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchNodePath {
    Full,
    PartialOrFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchedNodePath {
    Full {
        /// Number of path segments.
        ///
        /// Both the total and matched number of path segments are equals.
        number_of_segments: usize,
    },
    Partial {
        /// Number of matched path segments.
        ///
        /// Strictly less than the total number of path segments.
        number_of_matched_segments: NonZeroUsize,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedNodePath<'a, T>
where
    T: PathTreeTypes,
{
    pub node: &'a Arc<TreeNode<T>>,
    pub matched_path: MatchedNodePath,
}

impl<T: PathTreeTypes> PathTree<T> {
    /// Create a new path tree with the given root node.
    #[must_use]
    pub fn new(mut new_node_id: T::NewNodeId, root_node_value: NodeValue<T>) -> Self {
        let root_node_id = new_node_id.new_node_id();
        let root_node = TreeNode {
            id: root_node_id,
            parent: None,
            node: Node::from_value(root_node_value),
        };
        let mut nodes = new_hash_map();
        nodes.insert_mut(root_node_id, Arc::new(root_node));
        Self {
            root_node_id,
            new_node_id,
            nodes,
            _types: PhantomData,
        }
    }

    fn new_node_id(&mut self) -> T::NodeId {
        self.new_node_id.new_node_id()
    }

    #[must_use]
    pub const fn root_node_id(&self) -> T::NodeId {
        self.root_node_id
    }

    #[must_use]
    pub fn root_node(&self) -> &Arc<TreeNode<T>> {
        self.get_node(self.root_node_id)
    }

    #[must_use]
    pub fn lookup_node(&self, id: T::NodeId) -> Option<&Arc<TreeNode<T>>> {
        self.nodes.get(&id)
    }

    /// Get an existing node by its id.
    ///
    /// Only used internally for node ids that must exist. If the node does not exist
    /// the tree is probably in an inconsistent state!
    ///
    /// # Panics
    ///
    /// Panics if the node does not exist.
    #[must_use]
    fn get_node(&self, id: T::NodeId) -> &Arc<TreeNode<T>> {
        self.nodes.get(&id).expect("node exists")
    }

    /// Find a node by its path.
    #[must_use]
    #[cfg_attr(debug_assertions, allow(clippy::missing_panics_doc))] // Never panics
    pub fn find_node(&self, path: &T::RootPath) -> Option<&Arc<TreeNode<T>>> {
        self.resolve_node_path(path, MatchNodePath::Full).map(
            |ResolvedNodePath { node, matched_path }| {
                debug_assert_eq!(
                    matched_path,
                    MatchedNodePath::Full {
                        number_of_segments: path.segments().count()
                    }
                );
                node
            },
        )
    }

    #[must_use]
    pub fn contains_node(&self, node: &Arc<TreeNode<T>>) -> bool {
        self.lookup_node(node.id)
            .map_or(false, |tree_node| Arc::ptr_eq(tree_node, node))
    }

    /// Find a node by its path.
    ///
    /// Returns the found node and the number of resolved path segments.
    #[must_use]
    #[cfg_attr(debug_assertions, allow(clippy::missing_panics_doc))] // Never panics
    pub fn resolve_node_path(
        &self,
        path: &T::RootPath,
        match_path: MatchNodePath,
    ) -> Option<ResolvedNodePath<'_, T>> {
        // TODO: Use a trie data structure and Aho-Corasick algo for faster lookup?
        let root_node = self.get_node(self.root_node_id);
        let mut last_visited_node = root_node;
        let mut number_of_matched_path_segments = 0;
        let mut partial_path_match = false;
        for path_segment in path.segments() {
            debug_assert!(!path_segment.is_empty());
            match &last_visited_node.node {
                Node::Leaf(_) => {
                    // Path is too long, i.e. the remaining path segments could not be resolved.
                    match match_path {
                        MatchNodePath::Full => {
                            return None;
                        }
                        MatchNodePath::PartialOrFull => {
                            partial_path_match = true;
                            break;
                        }
                    }
                }
                Node::Inner(inner_node) => {
                    let child_node = inner_node
                        .children
                        .get(path_segment)
                        .map(|node_id| self.get_node(*node_id));
                    if let Some(child_node) = child_node {
                        last_visited_node = child_node;
                        number_of_matched_path_segments += 1;
                    } else {
                        // Path segment mismatch.
                        match match_path {
                            MatchNodePath::Full => {
                                return None;
                            }
                            MatchNodePath::PartialOrFull => {
                                partial_path_match = true;
                                break;
                            }
                        }
                    }
                }
            }
            debug_assert_eq!(
                path_segment,
                last_visited_node
                    .parent
                    .as_ref()
                    .expect("has parent")
                    .path_segment
                    .borrow()
            );
        }
        let matched_path = if partial_path_match {
            // At least 1 segment must match for a partial match.
            let number_of_matched_segments = NonZeroUsize::new(number_of_matched_path_segments)?;
            debug_assert!(number_of_matched_segments.get() < path.segments().count());
            MatchedNodePath::Partial {
                number_of_matched_segments,
            }
        } else {
            debug_assert_eq!(number_of_matched_path_segments, path.segments().count());
            MatchedNodePath::Full {
                number_of_segments: number_of_matched_path_segments,
            }
        };
        Some(ResolvedNodePath {
            node: last_visited_node,
            matched_path,
        })
    }

    fn create_missing_ancestor_nodes<'a>(
        &mut self,
        child_path: &'a T::RootPath,
        mut new_inner_value: impl FnMut() -> T::InnerValue,
        try_clone_leaf_into_inner_value: impl FnOnce(&T::LeafValue) -> Option<T::InnerValue>,
    ) -> Result<TreeNodeParentChildContext<'a, T>, TreeNodeParentChildPathConflict<T>> {
        if child_path.is_root() {
            return Ok(TreeNodeParentChildContext {
                parent_node: None,
                child_path_segment: None,
            });
        }
        let mut try_clone_leaf_into_inner_value = Some(try_clone_leaf_into_inner_value);
        let mut next_parent_node = Arc::clone(self.root_node());
        let (parent_path_segments, child_path_segment) = child_path.parent_child_segments();
        debug_assert!(child_path_segment.is_some());
        for path_segment in parent_path_segments {
            next_parent_node = match try_replace_leaf_with_inner_node(
                &mut self.nodes,
                next_parent_node,
                &mut try_clone_leaf_into_inner_value,
            ) {
                Ok(next_parent_node) => next_parent_node,
                Err(parent_node) => {
                    return Err(TreeNodeParentChildPathConflict {
                        parent_node,
                        child_path_segment: path_segment.to_owned(),
                    });
                }
            };
            let Node::Inner(inner_node) = &next_parent_node.node else {
                break;
            };
            let child_node = inner_node
                .children
                .get(path_segment)
                .map(|node_id| self.get_node(*node_id));
            if let Some(child_node) = child_node {
                log::debug!("Found child node {child_node:?} for path segment {path_segment:?}");
                next_parent_node = Arc::clone(child_node);
            } else {
                // Add new, empty inner node
                let child_node_id = self.new_node_id();
                debug_assert_ne!(child_node_id, next_parent_node.id);
                let child_node = TreeNode {
                    id: child_node_id,
                    parent: Some(HalfEdge {
                        path_segment: path_segment.to_owned(),
                        node_id: next_parent_node.id,
                    }),
                    node: Node::Inner(InnerNode::new(new_inner_value())),
                };
                log::debug!(
                    "Inserting new child node {child_node:?} for path segment {path_segment:?}"
                );
                let child_node = Arc::new(child_node);
                let new_next_parent_node = Arc::clone(&child_node);
                self.nodes.insert_mut(child_node.id, child_node);
                let mut inner_node = inner_node.clone();
                inner_node
                    .children
                    .insert_mut(path_segment.to_owned(), child_node_id);
                // Replace the parent node with the modified one
                let parent_node = TreeNode {
                    id: next_parent_node.id,
                    parent: next_parent_node.parent.clone(),
                    node: inner_node.into(),
                };
                self.nodes.insert_mut(parent_node.id, Arc::new(parent_node));
                next_parent_node = new_next_parent_node;
            }
            debug_assert_eq!(
                path_segment,
                next_parent_node
                    .parent
                    .as_ref()
                    .expect("has parent")
                    .path_segment
                    .borrow()
            );
        }
        let next_parent_node = match try_replace_leaf_with_inner_node(
            &mut self.nodes,
            next_parent_node,
            &mut try_clone_leaf_into_inner_value,
        ) {
            Ok(next_parent_node) => next_parent_node,
            Err(parent_node) => {
                return Err(TreeNodeParentChildPathConflict {
                    parent_node,
                    child_path_segment: child_path_segment
                        .expect("child path segment should exist")
                        .to_owned(),
                });
            }
        };
        let parent_node = match next_parent_node.node {
            Node::Inner(_) => Some(next_parent_node),
            Node::Leaf(_) => None,
        };
        Ok(TreeNodeParentChildContext {
            parent_node,
            child_path_segment,
        })
    }

    /// Insert or update a node in the tree.
    ///
    /// All missing parent nodes are created recursively and initialized
    /// with the value returned by `new_inner_value`.
    ///
    /// If the parent node exists and is a leaf node then it is replaced
    /// with an inner node by calling `try_clone_leaf_into_inner_value`.
    ///
    /// Returns the updated parent node and the inserted/updated child node.
    /// The parent node is `None` if the root node has been updated.
    ///
    /// In case of an error, the new value is returned back to the caller.
    #[allow(clippy::missing_panics_doc)] // Never panics
    pub fn insert_or_update_node_value(
        &mut self,
        path: &T::RootPath,
        new_value: NodeValue<T>,
        new_inner_value: &mut impl FnMut() -> T::InnerValue,
        try_clone_leaf_into_inner_value: impl FnOnce(&T::LeafValue) -> Option<T::InnerValue>,
    ) -> Result<ParentChildTreeNode<T>, InsertOrUpdateNodeValueError<T>> {
        let TreeNodeParentChildContext {
            parent_node,
            child_path_segment,
        } = match self.create_missing_ancestor_nodes(
            path,
            new_inner_value,
            try_clone_leaf_into_inner_value,
        ) {
            Ok(context) => context,
            Err(conflict) => {
                return Err(InsertOrUpdateNodeValueError::PathConflict {
                    conflict,
                    value: new_value,
                });
            }
        };
        let Some(parent_node) = parent_node else {
            // Update the root node.
            let old_root_node = Arc::clone(self.root_node());
            let new_root_node = self.update_node_value(&old_root_node, new_value)?;
            return Ok(ParentChildTreeNode {
                parent_node: None,
                child_node: new_root_node,
                replaced_child_node: Some(old_root_node),
            });
        };
        debug_assert!(matches!(parent_node.node, Node::Inner(_)));
        let child_path_segment = child_path_segment.expect("should never be empty");
        self.insert_or_update_child_node_value(&parent_node, child_path_segment, None, new_value)
    }

    /// Insert or update a child node in the tree.
    ///
    /// The parent node must exist and it must be an inner node.
    ///
    /// By providing `old_child_path_segment` an existing node could
    /// be renamed and updated. This will retain its `NodeId`.
    ///
    /// Returns the updated parent node and the inserted/updated child node.
    ///
    /// In case of an error, the new value is returned back to the caller.
    #[allow(clippy::missing_panics_doc)] // Never panics
    pub fn insert_or_update_child_node_value(
        &mut self,
        parent_node: &Arc<TreeNode<T>>,
        child_path_segment: &T::PathSegmentRef,
        old_child_path_segment: Option<&T::PathSegmentRef>,
        new_value: NodeValue<T>,
    ) -> Result<ParentChildTreeNode<T>, InsertOrUpdateNodeValueError<T>> {
        debug_assert!(self.contains_node(parent_node));
        debug_assert!(matches!(parent_node.node, Node::Inner(_)));
        let Node::Inner(inner_node) = &parent_node.node else {
            return Err(InsertOrUpdateNodeValueError::PathConflict {
                conflict: TreeNodeParentChildPathConflict {
                    parent_node: Arc::clone(parent_node),
                    child_path_segment: child_path_segment.to_owned(),
                },
                value: new_value,
            });
        };
        let old_child_path_segment = old_child_path_segment.unwrap_or(child_path_segment);
        if let Some(child_node) = inner_node
            .children
            .get(old_child_path_segment)
            .map(|node_id| self.get_node(*node_id))
        {
            log::debug!(
                "Updating value of existing child node {child_node_id}",
                child_node_id = child_node.id
            );
            let old_child_node = Arc::clone(child_node);
            let new_child_node = self.update_node_value(&old_child_node, new_value)?;
            return Ok(ParentChildTreeNode {
                parent_node: Some(Arc::clone(parent_node)),
                child_node: new_child_node,
                replaced_child_node: Some(old_child_node),
            });
        }
        let child_node_id = self.new_node_id();
        log::debug!("Adding new child node {child_node_id}");
        debug_assert!(!self.nodes.contains_key(&child_node_id));
        let new_child_node = TreeNode {
            id: child_node_id,
            parent: Some(HalfEdge {
                path_segment: child_path_segment.to_owned(),
                node_id: parent_node.id,
            }),
            node: Node::from_value(new_value),
        };
        let child_node_id = new_child_node.id;
        let new_child_node = Arc::new(new_child_node);
        self.nodes
            .insert_mut(child_node_id, Arc::clone(&new_child_node));
        log::debug!(
            "Inserted new child node {new_child_node:?}",
            new_child_node = *new_child_node,
        );
        let mut inner_node = inner_node.clone();
        if let Some(child_node_id_mut) = inner_node.children.get_mut(child_path_segment) {
            *child_node_id_mut = child_node_id;
        } else {
            inner_node
                .children
                .insert_mut(child_path_segment.to_owned(), child_node_id);
        }
        let parent_node = TreeNode {
            id: parent_node.id,
            parent: parent_node.parent.clone(),
            node: Node::Inner(inner_node),
        };
        let parent_node_id = parent_node.id;
        let new_parent_node = Arc::new(parent_node);
        self.nodes
            .insert_mut(parent_node_id, Arc::clone(&new_parent_node));
        Ok(ParentChildTreeNode {
            parent_node: Some(new_parent_node),
            child_node: new_child_node,
            replaced_child_node: None,
        })
    }

    /// Update a node value in the tree.
    ///
    /// Inner nodes with children could only be updated with an inner value.
    ///
    /// Returns the updated node with the new value.
    ///
    /// In case of an error, the new value is returned back to the caller.
    ///
    /// Undefined behavior if the given node does not belong to the tree.
    /// This precondition is only checked by debug assertions.
    #[allow(clippy::missing_panics_doc)] // Never panics
    pub fn update_node_value(
        &mut self,
        node: &Arc<TreeNode<T>>,
        new_value: NodeValue<T>,
    ) -> Result<Arc<TreeNode<T>>, UpdateNodeValueError<T>> {
        debug_assert!(self.contains_node(node));
        let new_node = Arc::new(node.try_clone_with_value(new_value)?);
        self.nodes.insert_mut(node.id, Arc::clone(&new_node));
        log::debug!("Updated node value: {node:?} -> {new_node:?}");
        Ok(new_node)
    }

    /// Remove a node and its children from the tree.
    ///
    /// Removes and returns the entire subtree rooted at the given node.
    ///
    /// The root node cannot be removed and the tree remains unchanged.
    ///
    /// Returns the removed subtree or `None` if unchanged.
    /// The node ids in the removed subtree remain unchanged.
    #[allow(clippy::missing_panics_doc)] // Never panics
    pub fn remove_subtree_by_id(&mut self, node_id: T::NodeId) -> Option<RemovedSubtree<T>> {
        if node_id == self.root_node_id {
            // Cannot remove the root node.
            return None;
        }
        let nodes_count_before = self.nodes_count();
        let node = self.nodes.get(&node_id).map(Arc::clone)?;
        let removed = self.nodes.remove_mut(&node_id);
        debug_assert!(removed);
        // The descendants of the removed node could still be collected,
        // even though the tree is already incomplete.
        let descendant_node_ids = node
            .node
            .descendants(self)
            .map(
                |HalfEdgeRef {
                     path_segment: _,
                     node_id,
                 }| node_id,
            )
            .collect::<Vec<_>>();
        // Split off the nodes of the subtree from the remaining nodes.
        let mut subtree_nodes: HashMap<_, _> = descendant_node_ids
            .into_iter()
            .filter_map(|node_id| {
                let node = self.nodes.get(&node_id).map(Arc::clone)?;
                let removed = self.nodes.remove_mut(&node_id);
                debug_assert!(removed);
                Some((node_id, node))
            })
            .collect();
        // Disconnect the subtree from the parent node. The old parent node
        // still references the root node of the removed subtree as a child.
        let new_parent_node = {
            debug_assert!(node.parent.is_some());
            let HalfEdge {
                path_segment: parent_path_segment,
                node_id: parent_node_id,
            } = node.parent.as_ref().expect("has parent");
            let parent_node = self.nodes.get(parent_node_id).expect("has a parent");
            debug_assert!(matches!(parent_node.node, Node::Inner(_)));
            let Node::Inner(inner_node) = &parent_node.node else {
                unreachable!();
            };
            let mut inner_node = inner_node.clone();
            debug_assert_eq!(
                inner_node
                    .children
                    .get(parent_path_segment.borrow())
                    .copied(),
                Some(node_id)
            );
            let removed = inner_node.children.remove_mut(parent_path_segment.borrow());
            debug_assert!(removed);
            TreeNode {
                id: parent_node.id,
                parent: parent_node.parent.clone(),
                node: Node::Inner(inner_node),
            }
        };
        let parent_node_id = new_parent_node.id;
        let new_parent_node = Arc::new(new_parent_node);
        self.nodes
            .insert_mut(parent_node_id, Arc::clone(&new_parent_node));
        // The tree is now back in a consistent state and we can use the public API again.
        let nodes_count_after = self.nodes_count();
        debug_assert!(nodes_count_before >= nodes_count_after);
        let removed_nodes_count = nodes_count_before - nodes_count_after;
        let TreeNode { id, parent, node } = Arc::unwrap_or_clone(node);
        let parent = parent.expect("has a parent");
        debug_assert_eq!(parent.node_id, new_parent_node.id);
        let child_path_segment = parent.path_segment;
        let subtree_root_node = Arc::new(TreeNode {
            id,
            parent: None,
            node,
        });
        subtree_nodes.insert_mut(node_id, subtree_root_node);
        let removed_subtree = Self {
            root_node_id: node_id,
            nodes: subtree_nodes,
            new_node_id: self.new_node_id.clone(),
            _types: PhantomData,
        };
        debug_assert_eq!(removed_nodes_count, removed_subtree.nodes_count());
        Some(RemovedSubtree {
            parent_node: new_parent_node,
            child_path_segment,
            removed_subtree,
        })
    }

    /// Insert a subtree.
    ///
    /// The root node of the subtree will replace an existing node.
    /// The existing node must node have any children, otherwise the
    /// insertion will fail.
    ///
    /// The inserted nodes from the subtree will be assigned new ids
    /// that are generated by this tree.
    ///
    /// By providing `old_child_path_segment` an existing node could
    /// be renamed and replaced by the subtree. This will retain its
    /// `NodeId`.
    ///
    /// Returns the new `NodeId` of the inserted/replaced node, i.e. the
    /// root node of the subtree.
    #[allow(clippy::missing_panics_doc)] // Never panics
    pub fn insert_or_replace_subtree(
        &mut self,
        parent_node: &Arc<TreeNode<T>>,
        child_path_segment: &T::PathSegmentRef,
        old_child_path_segment: Option<&T::PathSegmentRef>,
        mut subtree: Self,
    ) -> Result<T::NodeId, InsertOrUpdateNodeValueError<T>> {
        debug_assert!(self.contains_node(parent_node));
        let subtree_node_ids = std::iter::once(subtree.root_node_id())
            .chain(subtree.root_node().node.descendants(&subtree).map(
                |HalfEdgeRef {
                     path_segment: _,
                     node_id,
                 }| node_id,
            ))
            .collect::<Vec<_>>();
        let mut old_to_new_node_id =
            std::collections::HashMap::<T::NodeId, T::NodeId>::with_capacity(
                subtree_node_ids.len(),
            );
        // Will be replaced by the newly generated id.
        let mut new_subtree_root_node_id = subtree.root_node_id();
        for old_node_id in subtree_node_ids {
            let old_node = subtree
                .nodes
                .get(&old_node_id)
                .map(Arc::clone)
                .expect("node exists");
            let removed = subtree.nodes.remove_mut(&old_node_id);
            debug_assert!(removed);
            // Ideally, the nodes in the subtree are not referenced in the outer
            // context to avoid cloning them. For most use cases this assumption
            // should be valid.
            let TreeNode {
                id: _,
                parent,
                node,
            } = Arc::unwrap_or_clone(old_node);
            // TODO: This could be optimized when not reusing insert_or_update_child_node_value()
            // and instead inserting or replacing the node directly.
            let (parent_node, child_path_segment, old_child_path_segment) =
                if let Some(parent) = parent {
                    debug_assert!(old_to_new_node_id.contains_key(&parent.node_id));
                    let parent_node_id = old_to_new_node_id
                        .get(&parent.node_id)
                        .copied()
                        .expect("parent node has already been inserted");
                    let parent_node = self
                        .nodes
                        .get(&parent_node_id)
                        .expect("parent node has already been inserted");
                    (parent_node, parent.path_segment, None)
                } else {
                    // Root node.
                    debug_assert_eq!(old_node_id, subtree.root_node_id());
                    (
                        parent_node,
                        child_path_segment.to_owned(),
                        old_child_path_segment,
                    )
                };
            let node_value = match node {
                Node::Inner(inner) => NodeValue::Inner(inner.value),
                Node::Leaf(leaf) => NodeValue::Leaf(leaf.value),
            };
            let ParentChildTreeNode {
                parent_node: _,
                child_node,
                replaced_child_node: _,
            } = self
                .insert_or_update_child_node_value(
                    &Arc::clone(parent_node),
                    child_path_segment.borrow(),
                    old_child_path_segment,
                    node_value,
                )
                .inspect_err(|_| {
                    // Insertion could only fail for the first node,
                    // which is the root node of the subtree. This ensures
                    // that `self` remains unchanged on error.
                    debug_assert_eq!(old_node_id, subtree.root_node_id());
                })?;
            let new_node_id = child_node.id;
            debug_assert!(!old_to_new_node_id.contains_key(&old_node_id));
            old_to_new_node_id.insert(old_node_id, new_node_id);
            if old_node_id == subtree.root_node_id() {
                new_subtree_root_node_id = new_node_id;
            };
        }
        Ok(new_subtree_root_node_id)
    }

    /// Retain only the nodes that match the given predicate.
    ///
    /// The root node is always retained and cannot be removed.
    ///
    /// Returns the number of nodes that have been removed.
    #[allow(clippy::missing_panics_doc)] // Never panics
    pub fn retain_nodes(&mut self, mut predicate: impl FnMut(&TreeNode<T>) -> bool) {
        // TODO: Optimize by traversing the tree structure instead of iterating over
        // all nodes in no particular order. If a subtree is removed then all its
        // children don't need to be visited.
        let mut node_ids_to_remove = Vec::new();
        for node in self.nodes() {
            if !predicate(node) && node.id != self.root_node_id() {
                node_ids_to_remove.push(node.id);
            }
        }
        // Remove the subtrees in reverse order of the depth of their root node.
        node_ids_to_remove.sort_by(|lhs_id, rhs_id| {
            let lhs_node = self.get_node(*lhs_id);
            let rhs_node = self.get_node(*rhs_id);
            let lhs_depth = self.ancestor_nodes_count(lhs_node);
            let rhs_depth = self.ancestor_nodes_count(rhs_node);
            lhs_depth.cmp(&rhs_depth)
        });
        for node_id in node_ids_to_remove {
            self.remove_subtree_by_id(node_id);
        }
    }

    /// All nodes in no particular order.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = &Arc<TreeNode<T>>> {
        self.nodes.values()
    }

    /// Total number of nodes in the tree.
    ///
    /// In constant time, i.e. O(1).
    #[must_use]
    pub fn nodes_count(&self) -> usize {
        let node_count = self.nodes.size();
        // Verify invariants
        debug_assert_eq!(
            node_count,
            1 + self.root_node().node.descendants_count(self)
        );
        debug_assert_eq!(node_count, self.nodes().count());
        node_count
    }

    /// Iterator over all ancestor nodes of the given node.
    ///
    /// Returns the parent node and the respective path segment from the child node
    /// in bottom-up order.
    pub fn ancestor_nodes<'a>(
        &'a self,
        node: &'a Arc<TreeNode<T>>,
    ) -> impl Iterator<Item = HalfEdgeTreeNodeRef<'_, T>> + Clone {
        AncestorTreeNodeIter::new(self, node)
    }

    /// The number of parent nodes of the given node up to the root node.
    #[must_use]
    pub fn ancestor_nodes_count(&self, node: &Arc<TreeNode<T>>) -> usize {
        self.ancestor_nodes(node).count()
    }

    /// Returns an iterator over all descendants of this node
    ///
    /// Recursively traverses the subtree.
    ///
    /// The ordering of nodes is undefined and an implementation detail. Only parent
    /// nodes are guaranteed to be visited before their children.
    pub fn descendant_nodes<'a>(
        &'a self,
        node: &'a Arc<TreeNode<T>>,
    ) -> impl Iterator<Item = HalfEdgeRef<'a, T>> {
        debug_assert!(self.contains_node(node));
        node.node.descendants(self)
    }

    /// Number of child nodes of the given node (recursively).
    #[must_use]
    pub fn descendant_nodes_count(&self, node: &Arc<TreeNode<T>>) -> usize {
        debug_assert!(self.contains_node(node));
        node.node.descendants_count(self)
    }
}

/// Immutable node in the tree.
#[derive(Debug, Clone)]
pub struct TreeNode<T: PathTreeTypes> {
    /// Identifier for direct lookup.
    pub id: T::NodeId,

    /// Link to the parent node.
    ///
    /// Must be `None` for the root node and `Some` for all other nodes.
    pub parent: Option<HalfEdge<T>>,

    /// The actual content of this node.
    pub node: Node<T>,
}

impl<T: PathTreeTypes> TreeNode<T> {
    /// Clone the node with a new value.
    ///
    /// Leaf values could be replaced by both leaf and inner values.
    /// An inner value could only be replaced by a leaf value, if the
    /// node does not have any children.
    ///
    /// Fails if the type of the new value is incompatible with the
    /// current value type of the node, depending on its children.
    fn try_clone_with_value(
        &self,
        new_value: NodeValue<T>,
    ) -> Result<Self, UpdateNodeValueError<T>> {
        let new_node = match &self.node {
            Node::Inner(InnerNode { children, .. }) => {
                match new_value {
                    NodeValue::Inner(new_value) => {
                        // Remains an inner node with the current children and the new value.
                        Self {
                            id: self.id,
                            parent: self.parent.clone(),
                            node: Node::Inner(InnerNode {
                                children: children.clone(),
                                value: new_value,
                            }),
                        }
                    }
                    new_value @ NodeValue::Leaf(_) => {
                        if !children.is_empty() {
                            return Err(UpdateNodeValueError::ValueTypeMismatch {
                                value: new_value,
                            });
                        }
                        Self {
                            id: self.id,
                            parent: self.parent.clone(),
                            node: Node::from_value(new_value),
                        }
                    }
                }
            }
            Node::Leaf(_) => {
                // Leaf node values could be replaced by both leaf and inner node values.
                Self {
                    id: self.id,
                    parent: self.parent.clone(),
                    node: Node::from_value(new_value),
                }
            }
        };
        Ok(new_node)
    }
}

fn try_replace_leaf_with_inner_node<T: PathTreeTypes>(
    nodes: &mut HashMap<T::NodeId, Arc<TreeNode<T>>>,
    node: Arc<TreeNode<T>>,
    try_clone_leaf_into_inner_value: &mut Option<
        impl FnOnce(&T::LeafValue) -> Option<T::InnerValue>,
    >,
) -> Result<Arc<TreeNode<T>>, Arc<TreeNode<T>>> {
    let TreeNode {
        id,
        parent,
        node: Node::Leaf(LeafNode { value: leaf_value }),
    } = &*node
    else {
        return Ok(node);
    };
    let try_clone_leaf_into_inner_value = try_clone_leaf_into_inner_value
        .take()
        .expect("consumed at most once");
    let Some(inner_value) = try_clone_leaf_into_inner_value(leaf_value) else {
        // Keep this leaf node
        return Err(node);
    };
    // Replace leaf node with empty inner node
    let inner_node = TreeNode {
        id: *id,
        parent: parent.clone(),
        node: InnerNode::new(inner_value).into(),
    };
    log::debug!(
        "Replacing leaf node {leaf_node:?} with inner node {inner_node:?}",
        leaf_node = *node
    );
    let inner_node = Arc::new(inner_node);
    nodes.insert_mut(inner_node.id, Arc::clone(&inner_node));
    Ok(inner_node)
}

/// Iterator over all ancestor nodes of the given node.
///
/// Returns the node and the respective path segment from the child node.
#[derive(Debug, Clone)]
pub struct AncestorTreeNodeIter<'a, T: PathTreeTypes> {
    tree: &'a PathTree<T>,
    next_node: Option<&'a Arc<TreeNode<T>>>,
}

impl<'a, T: PathTreeTypes> AncestorTreeNodeIter<'a, T> {
    /// Create a new iterator over all ancestor nodes of the given node.
    ///
    /// The given node must exist in the tree. This is only checked in
    /// debug builds. Otherwise the iterator will be empty.
    #[must_use]
    pub fn new(tree: &'a PathTree<T>, node: &'a Arc<TreeNode<T>>) -> Self {
        debug_assert!(tree.contains_node(node));
        Self {
            tree,
            next_node: Some(node),
        }
    }
}

impl<'a, T: PathTreeTypes> Iterator for AncestorTreeNodeIter<'a, T> {
    type Item = HalfEdgeTreeNodeRef<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        let parent = self.next_node.as_ref()?.parent.as_ref()?;
        self.next_node = self.tree.lookup_node(parent.node_id);
        self.next_node.map(|node| HalfEdgeTreeNodeRef {
            path_segment: parent.path_segment.borrow(),
            node,
        })
    }
}
