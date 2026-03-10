//! Typed arena for cache-friendly allocation of Nodes and Pods.
//!
//! Uses generational indices to detect use-after-free at runtime.

use std::marker::PhantomData;

/// Generational index handle into an [`Arena`].
pub struct Handle<T> {
    pub index: u32,
    pub generation: u32,
    _marker: PhantomData<T>,
}

// Manual impls to avoid requiring bounds on T.
impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self { *self }
}
impl<T> Copy for Handle<T> {}
impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}
impl<T> Eq for Handle<T> {}
impl<T> std::hash::Hash for Handle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}
impl<T> std::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("index", &self.index)
            .field("generation", &self.generation)
            .finish()
    }
}

impl<T> Handle<T> {
    fn new(index: u32, generation: u32) -> Self {
        Self { index, generation, _marker: PhantomData }
    }
}

struct Entry<T> {
    value: Option<T>,
    generation: u32,
}

/// A typed arena with generational indices for O(1) insert/remove/lookup.
pub struct Arena<T> {
    entries: Vec<Entry<T>>,
    free: Vec<u32>,
    len: u32,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self { entries: Vec::new(), free: Vec::new(), len: 0 }
    }
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self { entries: Vec::with_capacity(cap), free: Vec::new(), len: 0 }
    }

    pub fn insert(&mut self, value: T) -> Handle<T> {
        self.len += 1;
        if let Some(idx) = self.free.pop() {
            let entry = &mut self.entries[idx as usize];
            entry.generation += 1;
            let gen = entry.generation;
            entry.value = Some(value);
            Handle::new(idx, gen)
        } else {
            let idx = self.entries.len() as u32;
            self.entries.push(Entry { value: Some(value), generation: 0 });
            Handle::new(idx, 0)
        }
    }

    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        let entry = self.entries.get_mut(handle.index as usize)?;
        if entry.generation != handle.generation || entry.value.is_none() {
            return None;
        }
        self.len -= 1;
        self.free.push(handle.index);
        entry.value.take()
    }

    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        let entry = self.entries.get(handle.index as usize)?;
        if entry.generation == handle.generation { entry.value.as_ref() } else { None }
    }

    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        let entry = self.entries.get_mut(handle.index as usize)?;
        if entry.generation == handle.generation { entry.value.as_mut() } else { None }
    }

    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterate over all live `(Handle<T>, &T)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.entries.iter().enumerate().filter_map(|(i, e)| {
            e.value.as_ref().map(|v| (Handle::new(i as u32, e.generation), v))
        })
    }

    /// Iterate over all live `(Handle<T>, &mut T)` pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Handle<T>, &mut T)> {
        self.entries.iter_mut().enumerate().filter_map(|(i, e)| {
            let gen = e.generation;
            e.value.as_mut().map(|v| (Handle::new(i as u32, gen), v))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_remove() {
        let mut arena: Arena<u64> = Arena::new();
        let h = arena.insert(42);
        assert_eq!(*arena.get(h).unwrap(), 42);
        assert_eq!(arena.remove(h), Some(42));
        assert!(arena.get(h).is_none());
    }

    #[test]
    fn generational_safety() {
        let mut arena: Arena<u64> = Arena::new();
        let h1 = arena.insert(1);
        arena.remove(h1);
        let h2 = arena.insert(2);
        // h1 is stale — same index, different generation
        assert!(arena.get(h1).is_none());
        assert_eq!(*arena.get(h2).unwrap(), 2);
    }
}
