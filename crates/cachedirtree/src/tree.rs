use super::utils::get_real_file_type;
use super::Options;
use bit_vec::BitVec;
use ego_tree::iter::Descendants;
use ego_tree::{NodeMut, NodeRef, Tree};
use std::fs;
use std::io;
use std::iter::{FromIterator, IntoIterator, Iterator, Skip};
use std::path::{Path, PathBuf};
use std::collections::BinaryHeap;
use std::time::SystemTime;

pub struct DirTree {
    tree: Tree<DirEntry>,
    recent: Option<Vec<DirEntryTimed>>
}

#[derive(Debug)]
pub struct DirEntry {
    pub name: String,
    pub search_tag: String,
}

impl DirEntry {
    pub fn new<S: ToString>(name: S) -> Self {
        let name: String = name.to_string();
        DirEntry {
            search_tag: name.to_lowercase(),
            name,
        }
    }
}

impl<T: ToString> From<T> for DirEntry {
    fn from(s: T) -> Self {
        DirEntry::new(s)
    }
}

#[derive(PartialEq, Eq)]
struct DirEntryTimed {
    path: PathBuf,
    created: SystemTime,
}

// need reverse ordering for heap, oldest will be on top
use std::cmp::Ordering;
impl PartialOrd for DirEntryTimed {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(match self.created.cmp(&other.created) {
            Ordering::Greater => Ordering::Less,
            Ordering::Less => Ordering::Greater,
            Ordering::Equal => self.path.cmp(&other.path),
        })
    }
}
impl Ord for DirEntryTimed {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(&other).unwrap()
    }
}

pub type DirRef<'a> = NodeRef<'a, DirEntry>;

pub struct SearchItem<'a>(DirRef<'a>);

impl<'a> SearchItem<'a> {
    pub fn path(&self) -> PathBuf {
        let segments: Vec<_> = self
            .0
            .ancestors()
            .filter_map(|n| {
                if n.parent().is_some() {
                    Some(&n.value().name)
                } else {
                    None
                }
            })
            .collect();
        let mut p = PathBuf::from_iter(segments.into_iter().rev());
        p.push(&self.0.value().name);
        p
    }

    pub fn name(&self) -> String {
        self.0.value().name.clone()
    }
}

pub struct SearchResult<'a> {
    current_node: DirRef<'a>,
    search_terms: Vec<String>,
    truncate_this_branch: bool,
    new_matched_terms: Option<BitVec>,
    matched_terms_stack: Vec<BitVec>,
}

impl<'a> Iterator for SearchResult<'a> {
    type Item = SearchItem<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(next) = if self.truncate_this_branch {
                None
            } else {
                self.current_node.first_child()
            } {
                self.current_node = next;
                match self.new_matched_terms.take() {
                    Some(n) => self.matched_terms_stack.push(n),
                    None => {
                        let c = self.matched_terms_stack.last().unwrap().clone();
                        self.matched_terms_stack.push(c)
                    }
                }
                trace!(
                    "Going down to {:?} - pushed {:?}",
                    self.current_node.value().name,
                    self.matched_terms_stack.last().unwrap()
                );
            } else if let Some(next) = self.current_node.next_sibling() {
                self.current_node = next;
                trace!("Going right to {:?}", self.current_node.value().name);
            } else if let Some(mut parent) = self.current_node.parent() {
                self.matched_terms_stack.pop().unwrap();
                trace!("Pop {:?}", self.matched_terms_stack.last().unwrap());
                while let None = parent.next_sibling() {
                    parent = match parent.parent() {
                        Some(p) => {
                            self.matched_terms_stack.pop().unwrap();
                            trace!("Pop {:?}", self.matched_terms_stack.last().unwrap());
                            p
                        }
                        None => return None,
                    };
                }
                // is safe to unwrap, as previous loop will either find parent with next sibling or return
                self.current_node = parent.next_sibling().unwrap();
                trace!(
                    "Going right after backtrack to {:?}",
                    self.current_node.value().name
                );
            } else {
                unreachable!("Never should run after root")
            }

            self.truncate_this_branch = false;
            if self.is_match() {
                // we already got match - we did not need to dive deaper
                trace!("returning match {:?}", self.current_node.value().name);
                self.truncate_this_branch = true;
                return Some(SearchItem(self.current_node));
            }
        }
    }
}

impl<'a> SearchResult<'a> {
    fn is_match(&mut self) -> bool {
        let mut matched = vec![];
        let mut res = true;
        let matched_terms = self.matched_terms_stack.last().unwrap();
        self.search_terms.iter().enumerate().for_each(|(i, term)| {
            if !matched_terms[i] {
                let contains = self.current_node.value().search_tag.contains(term);
                if contains {
                    matched.push(i)
                }
                res &= contains
            }
        });
        trace!(
            "Match  for terms {:?}, prev.matches {:?}, new matches {:?} res {:?}",
            self.search_terms,
            matched_terms,
            matched,
            res
        );
        if !res && !matched.is_empty() {
            let mut matched_terms = matched_terms.clone();
            matched.into_iter().for_each(|i| matched_terms.set(i, true));
            self.new_matched_terms = Some(matched_terms);
        } else {
            self.new_matched_terms  = None;
        }
        res
    }
}

impl<'a> IntoIterator for &'a DirTree {
    type Item = DirRef<'a>;
    type IntoIter = Skip<Descendants<'a, DirEntry>>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl DirTree {
    pub fn new<P: AsRef<Path>>(root_dir: P) -> Result<Self, io::Error> {
        DirTree::new_with_options(root_dir, Default::default())
    }

    pub fn new_with_options<P: AsRef<Path>>(root_dir: P, opts: Options) -> Result<Self, io::Error> {
        let p: &Path = root_dir.as_ref();
        let root_name = p.to_str().ok_or(io::Error::new(
            io::ErrorKind::Other,
            "root directory is not utf8",
        ))?;
        if !p.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "root path does not exists or is not director",
            ));
        }
        let mut cached = Tree::new(DirEntry::new(root_name));
        let mut recents = if opts.recent_list_size>0 {
            Some(BinaryHeap::with_capacity(opts.recent_list_size))
        } else {
            None
        };

        {
            let mut root = cached.root_mut();

            fn add_entries<P: AsRef<Path>>(
                node: &mut NodeMut<DirEntry>,
                root_dir: &Path,
                path: P,
                opts: &Options,
                mut recents: Option<&mut BinaryHeap<DirEntryTimed>>
            ) -> Result<(), io::Error> {
                for e in fs::read_dir(path)? {
                    let e = e?;
                    if let Ok(file_type) = get_real_file_type(&e, opts.follow_symlinks) {
                        if file_type.is_dir() {
                            let mut dir_node = node.append(e.file_name().to_string_lossy().into());
                            let p = e.path();
                            match recents.take() {
                                Some(r) => {
                                    if let Ok(meta) = p.metadata() {
                                    if let Ok(changed) = meta.modified() {
                                        if r.len() >= opts.recent_list_size {
                                            r.pop();
                                        }
                                        r.push(DirEntryTimed { path: p.strip_prefix(root_dir).unwrap().to_owned(), created:changed })
                                    }
                                    }
                                    add_entries(&mut dir_node, root_dir, &p, opts, Some(r))?;
                                    recents = Some(r);

                                }
                                None => {
                                    add_entries(&mut dir_node, root_dir, &p, opts, None)?;
                                }
                            }
                            
                        } else if opts.include_files && file_type.is_file() {
                            node.append(e.file_name().to_string_lossy().into());
                        }
                    }
                }
                Ok(())
            }

            add_entries(&mut root, p, p, &opts, recents.as_mut())?;
        }

        Ok(DirTree { tree: cached, recent: recents.map(|h| h.into_sorted_vec()) })
    }

    pub fn iter(&self) -> Skip<Descendants<DirEntry>> {
        self.tree.root().descendants().skip(1)
    }

    pub fn search<S: AsRef<str>>(&self, query: S) -> SearchResult {
        let search_terms = query
            .as_ref()
            .split(' ')
            .map(|s| s.trim().to_lowercase())
            .collect::<Vec<_>>();
        let m = BitVec::from_elem(search_terms.len(), false);
        SearchResult {
            new_matched_terms: None,
            matched_terms_stack: vec![m],
            current_node: self.tree.root(),
            search_terms,
            truncate_this_branch: false,
        }
    }

    pub fn recent(&self) -> Option<impl Iterator<Item=&Path>> {
        self.recent.as_ref().map(|v| v.iter().map(|e| e.path.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::OptionsBuilder;
    use super::*;
    #[test]
    fn test_creation() {
        let c = DirTree::new("test_data").unwrap();
        let root = c.iter();
        let num_children = root.count();
        assert!(num_children > 3);
        //c.iter().for_each(|n| println!("{}", n.value()))
    }

    #[test]
    fn test_search() {
        fn count_matches(mut s: SearchResult) -> usize {
            let mut num = 0;
            while let Some(n) = s.next() {
                println!("{:?}", n.path());
                num += 1;
            }
            num
        }
        let c = DirTree::new("test_data").unwrap();
        let s = c.search("usak");

        assert_eq!(0, count_matches(s));

        let s = c.search("target build");
        assert_eq!(2, count_matches(s));

        let s = c.search("cargo");
        assert_eq!(4, count_matches(s));
        let options = OptionsBuilder::default()
            .include_files(false)
            .build()
            .unwrap();
        let c = DirTree::new_with_options("test_data", options).unwrap();
        let s = c.search("cargo");
        assert_eq!(0, count_matches(s));
    }

    #[test]
    fn test_search2() {
        let c = DirTree::new("test_data").unwrap();
        let s = c.search("build target");
        assert_eq!(2, s.count());
    }

    #[test]
    fn test_search3() {
        let c = DirTree::new("test_data").unwrap();
        let s = c.search("doyle modry");
        assert_eq!(1, s.count());
        let s = c.search("chesterton modry");
        assert_eq!(1, s.count());
    }

    #[test]
    fn test_recent() {
        let options = OptionsBuilder::default()
            .recent_list_size(64)
            .build()
            .unwrap();
        let c = DirTree::new_with_options("test_data", options).unwrap();
        let recents: Vec<_> = c.recent().unwrap().collect();
        println!("Recents {:?}", recents);
        assert_eq!(9, recents.len());
        
    }

    
    #[test]
    fn test_search_symlinks() {
        env_logger::init();
        #[cfg(not(feature="symlinks"))]
        const NUM:usize = 0;
        #[cfg(feature="symlinks")]
        const NUM:usize = 1;
        let opts = OptionsBuilder::default()
            .follow_symlinks(true)
            .build()
            .unwrap();
        let c = DirTree::new_with_options("test_data", opts).unwrap();
        let s = c.search("doyle chesterton");
        assert_eq!(NUM, s.count());

        let c = DirTree::new("test_data").unwrap();
        let s = c.search("doyle chesterton");
        assert_eq!(0, s.count());
    }
}
