// SPDX-FileCopyrightText: The rpds-pathtree authors
// SPDX-License-Identifier: MPL-2.0

use std::{ffi::OsStr, fmt, hash::Hash};

/// Borrowed path segment.
pub trait PathSegment: Eq + Hash + fmt::Debug {
    /// Check if the segment is empty.
    #[must_use]
    fn is_empty(&self) -> bool;
}

impl PathSegment for str {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl PathSegment for OsStr {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

/// Decomposition of a path into segments.
pub trait SegmentedPath<S: PathSegment + ?Sized> {
    /// Iterate over all path segments.
    ///
    /// All segments are guaranteed to be non-empty.
    // TODO: How to avoid boxing the result?
    fn segments(&self) -> Box<dyn Iterator<Item = &S> + '_>;

    /// Split the path into parent segments and the last child segment.
    ///
    /// The returned iterator excludes the last segment that is
    /// included by [`Self::segments()`].
    // TODO: How to avoid boxing the result?
    fn parent_child_segments(&self) -> (Box<dyn Iterator<Item = &S> + '_>, Option<&S>);
}

/// [`SegmentedPath`] that is anchored at the root.
///
/// Implementing this trait asserts that the [segments](SegmentedPath::segments)
/// of the path are relative to the root of the tree, i.e. the path is absolute.
/// The root path itself has no segments.
pub trait RootPath<S: PathSegment + ?Sized>: SegmentedPath<S> {
    /// Check if the path equals the root path.
    ///
    /// The default implementation checks if the path has no segments.
    #[must_use]
    fn is_root(&self) -> bool {
        self.segments().next().is_none()
    }
}
