use std::{
    ops::{Index, IndexMut},
    path::{Component, Path, Prefix},
};

use chrono::{FixedOffset, TimeZone};
use git2::Time;
use lsp_types::{
    InlayHint, InlayHintKind, InlayHintLabel, Position, Uri, notification::Notification,
};

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

pub fn name_to_url(path: &Path) -> Option<Uri> {
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

pub fn markdown_language(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        // Systems
        Some("rs") => "rust",
        Some("c") => "c",
        Some("h") => "c",
        Some("cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx") => "cpp",
        Some("m") => "objective-c",
        Some("mm") => "objective-cpp",
        Some("zig") => "zig",
        Some("asm" | "s") => "asm",

        // JVM
        Some("java") => "java",
        Some("kt" | "kts") => "kotlin",
        Some("scala") => "scala",
        Some("groovy") => "groovy",

        // .NET
        Some("cs") => "csharp",
        Some("fs" | "fsx" | "fsi") => "fsharp",
        Some("vb") => "vbnet",

        // JavaScript / TypeScript
        Some("js" | "mjs" | "cjs") => "javascript",
        Some("jsx") => "jsx",
        Some("ts") => "typescript",
        Some("tsx") => "tsx",

        // Python / scripting
        Some("py" | "pyw" | "pyi") => "python",
        Some("rb") => "ruby",
        Some("php") => "php",
        Some("pl" | "pm") => "perl",
        Some("lua") => "lua",
        Some("r") => "r",
        Some("dart") => "dart",
        Some("ex" | "exs") => "elixir",
        Some("erl" | "hrl") => "erlang",
        Some("clj" | "cljs" | "cljc") => "clojure",
        Some("groovy") => "groovy",

        // Go / functional
        Some("go") => "go",
        Some("hs" | "lhs") => "haskell",
        Some("ml" | "mli") => "ocaml",
        Some("fs" | "fsx") => "fsharp",
        Some("swift") => "swift",

        // Web
        Some("html" | "htm") => "html",
        Some("css") => "css",
        Some("scss") => "scss",
        Some("sass") => "sass",
        Some("less") => "less",
        Some("vue") => "vue",
        Some("svelte") => "svelte",
        Some("astro") => "astro",

        // Data / config
        Some("json" | "jsonc") => "json",
        Some("yaml" | "yml") => "yaml",
        Some("toml") => "toml",
        Some("xml") => "xml",
        Some("ini") => "ini",
        Some("csv") => "csv",

        // Shell
        Some("sh" | "bash") => "bash",
        Some("zsh") => "zsh",
        Some("fish") => "fish",
        Some("ps1" | "psm1") => "powershell",
        Some("bat" | "cmd") => "bat",

        // SQL
        Some("sql") => "sql",

        // Mobile
        Some("swift") => "swift",
        Some("m") => "objective-c",
        Some("dart") => "dart",

        // Functional / misc
        Some("clj" | "cljs" | "cljc") => "clojure",
        Some("lisp" | "lsp" | "cl") => "lisp",
        Some("sol") => "solidity",
        Some("asm" | "s") => "asm",
        Some("v") => "verilog",
        Some("vhd" | "vhdl") => "vhdl",

        // Documentation / markup
        Some("md" | "markdown" | "mdx") => "markdown",
        Some("rst") => "rst",
        Some("tex") => "latex",

        // Default
        _ => "text",
    }
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
