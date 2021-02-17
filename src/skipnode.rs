use std::marker::PhantomData;
use std::{fmt, iter, ptr};

/// Minimum levels required for a list of size n.
pub fn levels_required(n: usize) -> usize {
    if n == 0 {
        1
    } else {
        let num_bits = std::mem::size_of::<usize>() * 8;
        num_bits - n.leading_zeros() as usize
    }
}

// ////////////////////////////////////////////////////////////////////////////
// SkipNode
// ////////////////////////////////////////////////////////////////////////////

/// SkipNodes are make up the SkipList.  The SkipList owns the first head-node
/// (which has no value) and each node has ownership of the next node through
/// `next`.
///
/// The node has a `level` which corresponds to how 'high' the node reaches.
///
/// A node of `level` n has (n + 1) links to next nodes, which are stored in
/// a vector.
///
/// The node linked by level 0 should be considered owned by this node.
///
/// There is a corresponding vector of link lengths which contains the distance
/// between current node and the next node. If there's no next node, the distance
/// is distance between current node and last reachable node.
///
/// Lastly, each node contains a link to the immediately previous node in case
/// one needs to parse the list backwards.
#[derive(Clone, Debug)]
pub struct SkipNode<V> {
    // key and value should never be None, with the sole exception being the
    // head node.
    pub value: Option<V>,
    // how high the node reaches.
    pub level: usize,
    // The immediately previous element.
    pub prev: *mut SkipNode<V>,
    // Vector of links to the next node at the respective level.  This vector
    // *must* be of length `self.level + 1`.  links[0] stores a pointer to the
    // next node, which will have to be dropped.
    pub links: Vec<*mut SkipNode<V>>,
    // The corresponding length of each link
    pub links_len: Vec<usize>,
    // Owns self.link[0]
    _phantom_link: PhantomData<SkipNode<V>>,
}

// ///////////////////////////////////////////////
// Inherent methods
// ///////////////////////////////////////////////

impl<V> SkipNode<V> {
    /// Create a new head node.
    pub fn head(total_levels: usize) -> Self {
        SkipNode {
            value: None,
            level: total_levels - 1,
            prev: ptr::null_mut(),
            links: iter::repeat(ptr::null_mut()).take(total_levels).collect(),
            links_len: iter::repeat(0).take(total_levels).collect(),
            _phantom_link: PhantomData,
        }
    }

    /// Create a new SkipNode with the given value.  The values of `prev` and
    /// `next` will all be `None` and have to be adjusted.
    pub fn new(value: V, level: usize) -> Self {
        SkipNode {
            value: Some(value),
            level,
            prev: ptr::null_mut(),
            links: iter::repeat(ptr::null_mut()).take(level + 1).collect(),
            links_len: iter::repeat(0).take(level + 1).collect(),
            _phantom_link: PhantomData,
        }
    }

    /// Consumes the node returning the value it contains.
    pub fn into_inner(mut self) -> Option<V> {
        self.value.take()
    }

    /// Returns `true` is the node is a head-node.
    pub fn is_head(&self) -> bool {
        self.prev.is_null()
    }

    pub fn next_ref(&self) -> Option<&Self> {
        unsafe { self.links[0].as_ref() }
    }

    pub fn next_mut(&mut self) -> Option<&mut Self> {
        unsafe { self.links[0].as_mut() }
    }

    /// Takes the next node and set next_node.prev as null.
    ///
    /// SAFETY: please make sure no link at level 1 or greater becomes dangling.
    pub unsafe fn take_next(&mut self) -> Option<Box<Self>> {
        let next = self.links[0];
        if next.is_null() {
            None
        } else {
            let mut next = Box::from_raw(next);
            next.prev = ptr::null_mut();
            self.links[0] = ptr::null_mut();
            self.links_len[0] = 0;
            Some(next)
        }
    }

    /// Replace the next node.
    /// Return the old node.
    ///
    /// SAFETY: please makes sure all links are fixed.
    pub unsafe fn replace_next(&mut self, mut new_next: Box<Self>) -> Option<Box<Self>> {
        let mut old_next = self.take_next();
        if let Some(old_next) = old_next.as_mut() {
            old_next.prev = ptr::null_mut();
        }
        new_next.prev = self as *mut _;
        self.links[0] = Box::into_raw(new_next);
        self.links_len[0] = 1;
        old_next
    }

    /// Distance between current node and the given node at specified level.
    /// If no node is given, then return distance between current node and the
    /// last possible node.
    /// If the node is not reachable on given level, return Err(()).
    pub fn distance(&self, level: usize, target: Option<&Self>) -> Result<usize, ()> {
        let distance = match target {
            Some(target) => {
                let (dest, distance) = self.advance_while(level, |current, _| {
                    current as *const _ != target as *const _
                });
                if dest as *const _ != target as *const _ {
                    return Err(());
                }
                distance
            }
            None => {
                let (dest, distance) = self.advance_while(level, |_, _| true);
                dest.links_len[level] + distance
            }
        };
        Ok(distance)
    }

    /// Try to move to next nth node at specified level.
    /// If it's impossible, then move as far as possible.
    /// Returns a reference to the new node and the distance travelled.
    pub fn advance_atmost(&self, level: usize, mut max_distance: usize) -> (&Self, usize) {
        self.advance_while(level, move |current_node, _| {
            let travelled = current_node.links_len[level];
            if travelled <= max_distance {
                max_distance -= travelled;
                return true;
            } else {
                return false;
            }
        })
    }

    /// Try to move to next nth node at specified level.
    /// If it's impossible, then move as far as possible.
    /// Returns a mutable reference to the new node and the distance travelled.
    pub fn advance_atmost_mut(
        &mut self,
        level: usize,
        mut max_distance: usize,
    ) -> (&mut Self, usize) {
        self.advance_while_mut(level, move |current_node, _| {
            let travelled = current_node.links_len[level];
            if travelled <= max_distance {
                max_distance -= travelled;
                return true;
            } else {
                return false;
            }
        })
    }

    pub fn advance_while(
        &self,
        level: usize,
        mut pred: impl FnMut(&Self, &Self) -> bool,
    ) -> (&Self, usize) {
        let mut current = self;
        let mut travelled = 0;
        loop {
            match current.advance_if(level, &mut pred) {
                Ok((node, steps)) => {
                    current = node;
                    travelled += steps;
                }
                Err(node) => return (node, travelled),
            }
        }
    }

    pub fn advance_while_mut(
        &mut self,
        level: usize,
        mut pred: impl FnMut(&Self, &Self) -> bool,
    ) -> (&mut Self, usize) {
        let mut current = self;
        let mut travelled = 0;
        loop {
            match current.advance_if_mut(level, &mut pred) {
                Ok((node, steps)) => {
                    current = node;
                    travelled += steps;
                }
                Err(node) => return (node, travelled),
            }
        }
    }

    // Due to Rust lifetime semantics, the lifetime of result is the same as self.
    // Sometimes Rust cannot determine the result is unused, e.g. in a loop.
    // As a result, self might be  borrowed forever and caller cannot return that value
    // if they call this function in a loop.
    // Therefore this function always return self when it fails to advance.
    pub fn advance_if_mut<'a>(
        &'a mut self,
        level: usize,
        predicate: impl FnOnce(&Self, &Self) -> bool,
    ) -> Result<(&'a mut Self, usize), &'a mut Self> {
        let next = unsafe { self.links[level].as_mut() };
        match next {
            Some(next) if predicate(self, next) => Ok((next, self.links_len[level])),
            _ => Err(self),
        }
    }

    pub fn advance_if<'a>(
        &'a self,
        level: usize,
        predicate: impl FnOnce(&Self, &Self) -> bool,
    ) -> Result<(&'a Self, usize), &'a Self> {
        let next = unsafe { self.links[level].as_mut() };
        match next {
            Some(next) if predicate(self, next) => Ok((next, self.links_len[level])),
            _ => Err(self),
        }
    }

    /// Find the node after distance units, then insert a new node after that node.
    pub fn insert<'a>(&'a mut self, new_node: Box<Self>, distance: usize) {
        let locater = {
            let mut distance_left = distance;
            move |node: &'a mut Self, level| {
                let (dest, distance) = node.advance_atmost_mut(level, distance_left);
                distance_left -= distance;
                (dest, distance)
            }
        };
        self._insert(self.level, new_node, locater);
    }

    /// Find the node after distance units, then remove the node after that node.
    pub fn remove<'a>(&'a mut self, distance: usize) -> Option<(Box<Self>, usize)> {
        let locater = {
            let mut distance_left = distance;
            move |node: &'a mut Self, level| {
                let (dest, distance) = node.advance_atmost_mut(level, distance_left);
                distance_left -= distance;
                if dest.links[0].is_null() {
                    None
                } else {
                    Some((dest, distance))
                }
            }
        };
        self._remove(self.level, locater)
    }

    /// Locater finds the node before the target position in a level,
    /// as well as the distance from input node to that node.
    ///
    /// Returns the reference to the new node, and distance between self and the new node.
    pub fn _insert<'a, F>(
        &'a mut self,
        level: usize,
        mut new_node: Box<Self>,
        mut locater: F,
    ) -> (&'a mut Self, usize)
    where
        F: FnMut(&'a mut Self, usize) -> (&'a mut Self, usize),
    {
        let (prev_node, prev_distance) = locater(self, level);
        let prev_node_p = prev_node as *mut Self;
        unsafe {
            if level == 0 {
                if let Some(tail) = prev_node.take_next() {
                    new_node.replace_next(tail);
                }
                prev_node.replace_next(new_node);
                return (prev_node.next_mut().unwrap(), prev_distance + 1);
            } else {
                let (inserted_node, insert_distance) =
                    prev_node._insert(level - 1, new_node, locater);
                if level <= inserted_node.level {
                    inserted_node.links[level] = (*prev_node_p).links[level];
                    inserted_node.links_len[level] =
                        (*prev_node_p).links_len[level] + 1 - insert_distance;
                    (*prev_node_p).links[level] = inserted_node as *mut _;
                    (*prev_node_p).links_len[level] = insert_distance;
                } else {
                    (*prev_node_p).links_len[level] += 1;
                }
                return (inserted_node, insert_distance + prev_distance);
            }
        }
    }

    pub fn _remove<'a, F>(&'a mut self, level: usize, mut locater: F) -> Option<(Box<Self>, usize)>
    where
        F: FnMut(&'a mut Self, usize) -> Option<(&'a mut Self, usize)>,
    {
        let (prev_node, prev_distance) = locater(self, level)?;
        let prev_node_p = prev_node as *mut Self;
        if level == 0 {
            // SAFETY: All links will be fixed later.
            let removed_node = unsafe {
                let mut removed_node = prev_node.take_next()?;
                if let Some(new_next) = removed_node.take_next() {
                    prev_node.replace_next(new_next);
                }
                removed_node
            };
            if let Some(next_node) = prev_node.next_mut() {
                next_node.prev = prev_node_p;
            }
            return Some((removed_node, prev_distance + 1));
        } else {
            let (removed_node, distance) = prev_node._remove(level - 1, locater)?;
            unsafe {
                if level <= removed_node.level {
                    (*prev_node_p).links[level] = removed_node.links[level];
                    assert_eq!((*prev_node_p).links_len[level], distance);
                    (*prev_node_p).links_len[level] = distance + removed_node.links_len[level] - 1;
                } else {
                    (*prev_node_p).links_len[level] -= 1;
                }
            }
            return Some((removed_node, prev_distance + distance));
        }
    }
}

impl<V> Drop for SkipNode<V> {
    fn drop(&mut self) {
        // SAFETY: all nodes are going to be dropped; its okay that its links (except those at
        // level 0) become dangling.
        unsafe {
            let mut node = self.take_next();
            while let Some(mut node_inner) = node {
                node = node_inner.take_next();
            }
        }
    }
}

// ///////////////////////////////////////////////
// Trait implementation
// ///////////////////////////////////////////////

impl<V> fmt::Display for SkipNode<V>
where
    V: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ref v) = self.value {
            write!(f, "{}", v)
        } else {
            Ok(())
        }
    }
}

// /////////////////////////////////
// Iterators
// /////////////////////////////////
// Since Iterators (currently) only pop from front and back,
// they can be shared by some data structures.
// There's no need for a dummy head (that contains no value) in the iterator.
// so the members are named first and last instaed of head/end to avoid confusion.

/// Consuming iterator.  
pub struct IntoIter<T> {
    pub(crate) first: Option<Box<SkipNode<T>>>,
    pub(crate) last: *mut SkipNode<T>,
    pub(crate) size: usize,
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        let mut popped_node = self.first.take()?;
        self.size -= 1;
        unsafe {
            self.first = if popped_node.links[0].is_null() {
                self.last = ptr::null_mut();
                None
            } else {
                let next_node = Box::from_raw(popped_node.links[0]);
                popped_node.links[0] = ptr::null_mut();
                Some(next_node)
            }
        }
        popped_node.into_inner()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.size, Some(self.size))
    }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<T> {
        if self.first.is_none() {
            return None;
        }
        assert!(!self.last.is_null());
        unsafe {
            let new_last = (*self.last).prev;
            let popped_node = if new_last.is_null() {
                self.first.take().unwrap()
            } else {
                let popped_node = (*new_last).links[0];
                (*new_last).links[0] = ptr::null_mut();
                let popped_node = Box::from_raw(popped_node);
                popped_node
            };
            self.last = new_last;
            self.size -= 1;
            popped_node.into_inner()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_level_required() {
        assert_eq!(levels_required(0), 1);
        assert_eq!(levels_required(1), 1);
        assert_eq!(levels_required(2), 2);
        assert_eq!(levels_required(3), 2);
        assert_eq!(levels_required(1023), 10);
        assert_eq!(levels_required(1024), 11);
    }

    fn level_for_index(mut n: usize) -> usize {
        let mut cnt = 0;
        while n & 0x1 == 1 {
            cnt += 1;
            n /= 2;
        }
        cnt
    }

    #[test]
    fn test_level_index() {
        assert_eq!(level_for_index(0), 0);
        assert_eq!(level_for_index(1), 1);
        assert_eq!(level_for_index(2), 0);
        assert_eq!(level_for_index(3), 2);
        assert_eq!(level_for_index(4), 0);
        assert_eq!(level_for_index(5), 1);
        assert_eq!(level_for_index(6), 0);
        assert_eq!(level_for_index(7), 3);
        assert_eq!(level_for_index(8), 0);
        assert_eq!(level_for_index(9), 1);
        assert_eq!(level_for_index(10), 0);
        assert_eq!(level_for_index(11), 2);
    }

    /// Make a list of size n
    /// levels are evenly spread out
    fn new_list_for_test(n: usize) -> SkipNode<usize> {
        let max_level = levels_required(n);
        let mut head = SkipNode::<usize>::head(max_level);
        assert_eq!(head.links.len(), max_level);
        let mut nodes: Vec<_> = (0..n)
            .map(|n| {
                let new_node = Box::new(SkipNode::new(n, level_for_index(n)));
                Box::into_raw(new_node)
            })
            .collect();
        unsafe {
            let node_max_level = nodes.iter().map(|&node| (*node).level).max();
            if let Some(node_max_level) = node_max_level {
                assert_eq!(node_max_level + 1, max_level);
            }
            for level in 0..max_level {
                let mut last_node = &mut head as *mut SkipNode<usize>;
                let mut len_left = n;
                for &mut node_ptr in nodes
                    .iter_mut()
                    .filter(|&&mut node_ptr| level <= (*node_ptr).level)
                {
                    if level == 0 {
                        (*node_ptr).prev = last_node;
                    }
                    (*last_node).links[level] = node_ptr;
                    (*last_node).links_len[level] = 1 << level;
                    last_node = node_ptr;
                    len_left -= 1 << level;
                }
                (*last_node).links_len[level] = len_left;
            }
        }
        return head;
    }

    #[test]
    fn test_make_new_list() {
        fn test_list_integrity(len: usize) {
            let list = new_list_for_test(len);
            unsafe {
                let mut node = list.links[0].as_ref();
                while let Some(node_inner) = node {
                    let idx = node_inner.value.unwrap_or_else(|| panic!());
                    assert_eq!(node_inner.level, level_for_index(idx));
                    node = node_inner.links[0].as_ref();
                }

                for level in 0..levels_required(len) {
                    let mut len_left = len;
                    let mut node = &list;
                    while let Some(next_node) = node.links[level].as_ref() {
                        len_left -= node.links_len[level];
                        node = next_node;
                    }
                    assert_eq!(len_left, node.links_len[level])
                }
            }
        }
        test_list_integrity(0);
        test_list_integrity(1);
        test_list_integrity(2);
        test_list_integrity(3);
        test_list_integrity(10);
        test_list_integrity(1023);
        test_list_integrity(1024);
        test_list_integrity(1025);
    }
}
