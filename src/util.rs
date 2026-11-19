use std::{
    ops::{Index, IndexMut},
    path::{Component, Path, Prefix},
};

use chrono::{FixedOffset, TimeZone};
use git2::Time;
use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position, SemanticToken, Uri};

pub struct Container<T> {
    inner: Vec<T>,
}

impl<T> Container<T> {
    pub fn new() -> Self {
        Self { inner: vec![] }
    }

    pub fn add(&mut self, item: T) -> usize {
        self.inner.push(item);
        self.inner.len() - 1
    }

    pub fn push(&mut self, item: T) {
        self.inner.push(item);
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

impl<T> Index<&mut usize> for Container<T> {
    type Output = T;
    fn index(&self, index: &mut usize) -> &Self::Output {
        &self.inner[*index]
    }
}

impl<T> IndexMut<&mut usize> for Container<T> {
    fn index_mut(&mut self, index: &mut usize) -> &mut Self::Output {
        &mut self.inner[*index]
    }
}

impl<T> Index<&usize> for Container<T> {
    type Output = T;
    fn index(&self, index: &usize) -> &Self::Output {
        &self.inner[*index]
    }
}

fn new_name_to_url(path: &Path) -> Option<lsp_types::Uri> {
    let url = url::Url::from_file_path(path).ok()?;
    let uri = url.as_str().parse().ok()?;
    Some(uri)
}

pub fn name_to_url(path: &Path) -> Option<Uri> {
    return new_name_to_url(path);
    //let path = Path::new(name);
    if !path.is_absolute() {
        return None;
    }
    let mut raw = String::from("file://");
    for component in path.components() {
        match component {
            Component::Normal(seg) => {
                raw.push('/');
                raw.push_str(&component.as_os_str().to_string_lossy());
            }
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    raw.push('/');
                    raw.push(letter as char);
                    raw.push(':');
                }
                _ => {
                    raw.push('/');
                    raw.push_str(&prefix.as_os_str().to_string_lossy());
                }
            },
            Component::RootDir | Component::CurDir | Component::ParentDir => {}
        }
    }
    raw.parse().ok()
}

pub fn format_git_time(time: Time) -> Option<String> {
    if let Some(offset) = FixedOffset::east_opt(time.offset_minutes() * 60) {
        if let Some(dt) = offset.timestamp_opt(time.seconds(), 0).single() {
            return Some(dt.format("%a %b %d %H:%M:%S %Y %z").to_string());
        }
    }
    None
}

pub fn create_inlay_hint(line: u32, character: u32, label: impl Into<String>) -> InlayHint {
    InlayHint {
        position: Position::new(line, character),
        label: InlayHintLabel::String(label.into()),
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: None,
        padding_right: None,
        data: None,
    }
}

pub fn create_semantic_token(
    delta_line: impl Into<u32>,
    delta_start: impl Into<u32>,
    length: impl Into<u32>,
    token_type: impl Into<u32>,
) -> SemanticToken {
    SemanticToken {
        delta_line: delta_line.into(),
        delta_start: delta_start.into(),
        length: length.into(),
        token_type: token_type.into(),
        token_modifiers_bitset: 0,
    }
}
