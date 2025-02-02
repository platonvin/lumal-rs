// vector that has index that moves by one untile reaches the end and then wraps
// primarly used for CPU-GPU resources, where GPU operates on previous frame resources, and CPU operates on current (frame resources)

use crate::MAX_FRAMES_IN_FLIGHT;
use std::ops::{Index, IndexMut}; // lol

#[derive(Clone, Debug, Default)]
pub struct Ring<T: Default + Clone> {
    pub data: Vec<T>,
    pub index: usize,
}

impl<T: std::default::Default + Clone> Ring<T> {
    /// Creates a new `Ring` with a given size and initial value for all elements.
    pub fn new(size: usize, initial_value: T) -> Self
    where
        T: Clone,
    {
        Self {
            data: vec![initial_value; size],
            index: 0,
        }
    }
    pub fn from_vec(data: Vec<T>) -> Self {
        Self { data, index: 0 }
    }
    pub fn resize(&mut self, size: usize, initial_value: T) {
        self.data.resize(size, initial_value);
        if self.data.len() > self.index {
            self.index = self.data.len() - 1;
        }
    }

    /// Returns the current element in the Ring.
    pub fn current(&self) -> &T {
        &self.data[self.index]
    }
    pub fn previous(&self) -> &T {
        let index = self.index + self.data.len() - 1;
        let wrapped_index = index % self.data.len();
        &self.data[wrapped_index]
    }
    pub fn next(&self) -> &T {
        let index = self.index + 1;
        let wrapped_index = index % self.data.len();
        &self.data[wrapped_index]
    }

    /// Mutably access the current element in the Ring.
    pub fn current_mut(&mut self) -> &mut T {
        &mut self.data[self.index]
    }

    /// Moves to the next element in the Ring (circularly).
    pub fn move_next(&mut self) {
        self.index = (self.index + 1) % self.data.len();
    }

    /// Moves to the previous element in the Ring (circularly).
    pub fn move_previous(&mut self) {
        if self.index == 0 {
            self.index = self.data.len() - 1;
        } else {
            self.index -= 1;
        }
    }

    /// Access an element by absolute index (circularly).
    pub fn get(&self, idx: usize) -> &T {
        &self.data[idx % self.data.len()]
    }

    /// Mutably access an element by absolute index (circularly).
    pub fn get_mut(&mut self, idx: usize) -> &mut T {
        let len = &self.data.len();
        &mut self.data[idx % len]
    }

    /// Resets the index to zero.
    pub fn reset_index(&mut self) {
        self.index = 0;
    }

    /// Returns the length of the Ring.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Checks if the Ring is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn as_mut_ptr(&mut self) -> *mut Ring<T> {
        self
    }

    pub fn as_mut_ref(&self) -> *const () {
        self as *const Ring<T> as *const ()
    }

    pub fn as_ptr(&self) -> *const () {
        self as *const Ring<T> as *const ()
    }

    pub fn as_slice(&self) -> &[T] {
        self.data.as_slice()
    }

    pub fn iter(&self) -> RingIterator<T> {
        RingIterator {
            ring: self,
            position: 0,
        }
    }

    pub fn first(&self) -> &T {
        &self.data[0]
    }
}

/// Implement `Index` for read-only access using square brackets.
impl<T: std::default::Default + Clone> Index<usize> for Ring<T> {
    type Output = T;

    fn index(&self, idx: usize) -> &Self::Output {
        self.get(idx)
    }
}

/// Implement `IndexMut` for mutable access using square brackets.
impl<T: std::default::Default + Clone> IndexMut<usize> for Ring<T> {
    fn index_mut(&mut self, idx: usize) -> &mut Self::Output {
        self.get_mut(idx)
    }
}

/// Iterator for `Ring`.
pub struct RingIterator<'a, T: Default + Clone> {
    ring: &'a Ring<T>,
    position: usize,
}

impl<'a, T: std::default::Default + Clone> IntoIterator for &'a Ring<T> {
    type Item = &'a T;
    type IntoIter = RingIterator<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        RingIterator {
            ring: self,
            position: 0,
        }
    }
}

impl<'a, T: std::default::Default + Clone> Iterator for RingIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position < self.ring.len() {
            let item = &self.ring.data[self.position];
            self.position += 1;
            Some(item)
        } else {
            None
        }
    }
}

impl<'a, T: std::default::Default + Clone> FromIterator<T> for Ring<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut data = Vec::new();
        for item in iter {
            data.push(item);
        }
        Self { data, index: 0 }
    }
}
