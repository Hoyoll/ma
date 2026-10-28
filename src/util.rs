use std::ops::{Index, IndexMut};

use lsp_types::notification::Notification;

pub struct Container<T> {
    inner: Vec<T>
}

impl<T> Container<T> {
    pub fn new() -> Self {
        Self { inner: vec![] }
    }

    pub fn add(&mut self, item: T) -> usize {
        self.inner.push(item);
        self.inner.len() - 1
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.inner.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.inner.get_mut(index)
    }
    
    pub fn inner(&self) -> &Vec<T> {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut Vec<T> {
        &mut self.inner
    }
}

impl<T> IndexMut<usize> for Container<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.inner[index]
    }
}

impl<T> Index<usize> for Container<T> {
    type Output = T;
    fn index(&self, index: usize) -> &Self::Output {
        &self.inner[index]
    }
}

impl<T> IndexMut<&usize> for Container<T> {
    fn index_mut(&mut self, index: &usize) -> &mut Self::Output {
        &mut self.inner[*index]
    }
}

impl<T> Index<&usize> for Container<T> {
    type Output = T;
    fn index(&self, index: &usize) -> &Self::Output {
        &self.inner[*index]
    }
}
