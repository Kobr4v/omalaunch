// SPDX-License-Identifier: GPL-3.0-or-later
//! Library model: pure data + selection/filter logic, no GTK types.
//! Everything here is headless-testable; `view` renders it.

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    pub comment: String,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub icon_path: Option<PathBuf>,
    pub update_available: bool,
    /// Seconds since epoch (filesystem mtime); drives Recent sort.
    pub mtime: u64,
    pub favorite: bool,
    pub hidden: bool,
    pub db_id: Option<i64>,
    pub play_count: i64,
}

/// Match quality tier, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchTier {
    NamePrefix,
    Substring,
    Fuzzy,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    UpdateFirst,
    Name,
    Recent,
    Played,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    List,
    Grid,
}

#[derive(Debug, Default)]
pub struct LibraryModel {
    items: Vec<Item>,
    filter: String,
    selected: Option<usize>,
    sort: SortOrder,
    selected_category: Option<String>,
    view_mode: ViewMode,
    favorites_only: bool,
    updates_only: bool,
    show_hidden: bool,
}

impl LibraryModel {
    pub fn new(items: Vec<Item>) -> Self {
        let mut model = Self {
            items,
            filter: String::new(),
            selected: None,
            sort: SortOrder::default(),
            selected_category: None,
            view_mode: ViewMode::List,
            favorites_only: false,
            updates_only: false,
            show_hidden: false,
        };
        model.sort_items();
        model.select_first();
        model
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_lowercase();
        self.select_first();
    }

    pub fn set_sort(&mut self, sort: SortOrder) {
        let keep = self.selected_item().map(|i| i.path.clone());
        self.sort = sort;
        self.sort_items();
        self.selected = None;
        if let Some(path) = keep {
            let visible = self.filtered_indices();
            self.selected = visible.iter().position(|i| self.items[*i].path == path);
        }
        if self.selected.is_none() {
            self.select_first();
        }
    }

    pub fn sort(&self) -> SortOrder {
        self.sort
    }

    pub fn set_category(&mut self, category: Option<String>) {
        self.selected_category = category;
        self.select_first();
    }

    /// Union of categories and tags, sorted, for the facet dropdown.
    pub fn available_facets(&self) -> Vec<String> {
        let mut facets: Vec<String> = self
            .items
            .iter()
            .flat_map(|i| i.categories.iter().chain(i.tags.iter()).cloned())
            .collect();
        facets.sort();
        facets.dedup();
        facets
    }

    pub fn match_tier(&self, item: &Item) -> MatchTier {
        if self.filter.is_empty() {
            return MatchTier::Substring;
        }
        let name = item.name.to_lowercase();
        if name.starts_with(&self.filter) {
            return MatchTier::NamePrefix;
        }
        let haystacks = [
            name,
            item.comment.to_lowercase(),
            item.path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase(),
        ];
        if haystacks.iter().any(|h| h.contains(&self.filter)) {
            return MatchTier::Substring;
        }
        let matcher = SkimMatcherV2::default();
        if haystacks
            .iter()
            .any(|h| matcher.fuzzy_match(h, &self.filter).is_some())
        {
            return MatchTier::Fuzzy;
        }
        MatchTier::None
    }

    fn matches(&self, item: &Item) -> bool {
        if !self.show_hidden && item.hidden {
            return false;
        }
        if self.favorites_only && !item.favorite {
            return false;
        }
        if self.updates_only && !item.update_available {
            return false;
        }
        if let Some(facet) = &self.selected_category {
            if !item.categories.iter().any(|c| c == facet) && !item.tags.iter().any(|t| t == facet)
            {
                return false;
            }
        }
        self.match_tier(item) != MatchTier::None
    }

    pub fn set_favorites_only(&mut self, only: bool) {
        self.favorites_only = only;
        self.select_first();
    }

    pub fn set_updates_only(&mut self, only: bool) {
        self.updates_only = only;
        self.select_first();
    }

    pub fn set_show_hidden(&mut self, show: bool) {
        self.show_hidden = show;
        self.select_first();
    }

    fn sort_items(&mut self) {
        use std::cmp::Reverse;
        match self.sort {
            SortOrder::UpdateFirst => self.items.sort_by(|a, b| {
                b.update_available
                    .cmp(&a.update_available)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }),
            SortOrder::Name => self.items.sort_by_key(|a| a.name.to_lowercase()),
            SortOrder::Recent => self.items.sort_by_key(|b| Reverse(b.mtime)),
            SortOrder::Played => self.items.sort_by(|a, b| {
                b.play_count
                    .cmp(&a.play_count)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }),
        }
    }

    // Consumed by search/sort + keyboard map (todos 16/18); tests pin behavior now.
    #[allow(dead_code)]
    pub fn filter(&self) -> &str {
        &self.filter
    }

    fn filtered_indices(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| self.matches(item))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn visible(&self) -> Vec<&Item> {
        let indices = self.filtered_indices();
        indices.into_iter().map(|i| &self.items[i]).collect()
    }

    // Consumed by search/sort + keyboard map (todos 16/18); tests pin behavior now.
    #[allow(dead_code)]
    pub fn visible_count(&self) -> usize {
        self.filtered_indices().len()
    }

    pub fn selected_visible_index(&self) -> Option<usize> {
        self.selected
    }

    pub fn select_visible_index(&mut self, index: usize) {
        if index < self.filtered_indices().len() {
            self.selected = Some(index);
        }
    }

    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
    }

    /// Replace the item set (e.g. after integrating), preserving selection
    /// by path when the selected item still exists.
    pub fn replace_items(&mut self, items: Vec<Item>) {
        let keep = self.selected_item().map(|i| i.path.clone());
        self.items = items;
        self.sort_items();
        self.selected = None;
        if let Some(path) = keep {
            let visible = self.filtered_indices();
            self.selected = visible.iter().position(|i| self.items[*i].path == path);
        }
        if self.selected.is_none() {
            self.select_first();
        }
    }

    pub fn selected_item(&self) -> Option<&Item> {
        let visible = self.filtered_indices();
        self.selected
            .and_then(|s| visible.get(s).map(|i| &self.items[*i]))
    }

    pub fn select_next(&mut self) {
        let n = self.filtered_indices().len();
        if n == 0 {
            self.selected = None;
            return;
        }
        self.selected = Some(self.selected.map_or(0, |s| (s + 1) % n));
    }

    pub fn select_prev(&mut self) {
        let n = self.filtered_indices().len();
        if n == 0 {
            self.selected = None;
            return;
        }
        self.selected = Some(self.selected.map_or(0, |s| (s + n - 1) % n));
    }

    pub fn select_first(&mut self) {
        self.selected = (!self.filtered_indices().is_empty()).then_some(0);
    }

    pub fn select_last(&mut self) {
        let n = self.filtered_indices().len();
        self.selected = if n == 0 { None } else { Some(n - 1) };
    }

    pub fn move_by(&mut self, delta: i32) {
        let n = self.filtered_indices().len();
        if n == 0 {
            self.selected = None;
            return;
        }
        let current = self.selected.unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, n as i32 - 1) as usize;
        self.selected = Some(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        path: &str,
        name: &str,
        comment: &str,
        categories: &[&str],
        update: bool,
        mtime: u64,
    ) -> Item {
        Item {
            path: PathBuf::from(path),
            name: name.to_string(),
            comment: comment.to_string(),
            categories: categories.iter().map(|s| s.to_string()).collect(),
            tags: Vec::new(),
            icon_path: None,
            update_available: update,
            mtime,
            favorite: false,
            hidden: false,
            db_id: None,
            play_count: 0,
        }
    }

    fn items() -> Vec<Item> {
        vec![
            item(
                "/a/Zebra.AppImage",
                "Zebra",
                "striped",
                &["Graphics"],
                false,
                30,
            ),
            item(
                "/a/Alpha.AppImage",
                "Alpha",
                "first",
                &["Utility"],
                true,
                10,
            ),
            item(
                "/a/Mike.AppImage",
                "Mike",
                "middle",
                &["Utility", "Audio"],
                false,
                20,
            ),
        ]
    }

    #[test]
    fn sorts_updates_first_then_name() {
        let model = LibraryModel::new(items());
        let names: Vec<&str> = model.visible().iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "Mike", "Zebra"]);
    }

    #[test]
    fn filter_matches_name_or_comment() {
        let mut model = LibraryModel::new(items());
        model.set_filter("strip");
        assert_eq!(model.visible_count(), 1);
        assert_eq!(
            model.selected_item().map(|i| i.name.as_str()),
            Some("Zebra")
        );
        model.set_filter("zzz-no-match");
        assert_eq!(model.visible_count(), 0);
        assert_eq!(model.selected_item(), None);
    }

    #[test]
    fn jump_and_page_navigation() {
        let mut model = LibraryModel::new(items());
        model.select_last();
        assert_eq!(
            model.selected_item().map(|i| i.name.as_str()),
            Some("Zebra")
        );
        model.move_by(-10);
        assert_eq!(
            model.selected_item().map(|i| i.name.as_str()),
            Some("Alpha")
        );
        model.move_by(10);
        assert_eq!(
            model.selected_item().map(|i| i.name.as_str()),
            Some("Zebra")
        );
        model.move_by(1);
        assert_eq!(
            model.selected_item().map(|i| i.name.as_str()),
            Some("Zebra")
        );
    }

    #[test]
    fn navigation_wraps() {
        let mut model = LibraryModel::new(items());
        assert_eq!(
            model.selected_item().map(|i| i.name.as_str()),
            Some("Alpha")
        );
        model.select_next();
        assert_eq!(model.selected_item().map(|i| i.name.as_str()), Some("Mike"));
        model.select_prev();
        assert_eq!(
            model.selected_item().map(|i| i.name.as_str()),
            Some("Alpha")
        );
        model.select_prev();
        assert_eq!(
            model.selected_item().map(|i| i.name.as_str()),
            Some("Zebra")
        );
    }

    #[test]
    fn empty_library_state() {
        let model = LibraryModel::new(vec![]);
        assert!(model.is_empty());
        assert_eq!(model.selected_item(), None);
    }

    #[test]
    fn direct_index_selection_and_view_mode() {
        let mut model = LibraryModel::new(items());
        assert_eq!(model.view_mode(), ViewMode::List);
        model.select_visible_index(2);
        assert_eq!(
            model.selected_item().map(|i| i.name.as_str()),
            Some("Zebra")
        );
        model.select_visible_index(99);
        assert_eq!(
            model.selected_item().map(|i| i.name.as_str()),
            Some("Zebra")
        );
        model.set_view_mode(ViewMode::Grid);
        assert_eq!(model.view_mode(), ViewMode::Grid);
    }

    #[test]
    fn match_tiers_rank_exact_over_fuzzy() {
        let model = LibraryModel::new(items());
        let by_name = |n: &str| items().into_iter().find(|i| i.name == n).expect("item");
        let mut prefixed = LibraryModel::new(items());
        prefixed.set_filter("alp");
        assert_eq!(
            prefixed.match_tier(&by_name("Alpha")),
            MatchTier::NamePrefix
        );
        let mut fuzzy = LibraryModel::new(items());
        fuzzy.set_filter("zba");
        assert_eq!(fuzzy.match_tier(&by_name("Zebra")), MatchTier::Fuzzy);
        assert_eq!(fuzzy.match_tier(&by_name("Alpha")), MatchTier::None);
        assert_eq!(model.match_tier(&by_name("Mike")), MatchTier::Substring);
    }

    #[test]
    fn sort_orders() {
        let mut model = LibraryModel::new(items());
        model.set_sort(SortOrder::Name);
        let names: Vec<&str> = model.visible().iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "Mike", "Zebra"]);
        model.set_sort(SortOrder::Recent);
        let names: Vec<&str> = model.visible().iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Zebra", "Mike", "Alpha"]);
        // Selection survives re-sort when still visible.
        model.set_filter("mike");
        model.set_sort(SortOrder::Name);
        assert_eq!(model.selected_item().map(|i| i.name.as_str()), Some("Mike"));
    }

    #[test]
    fn played_sort_and_tag_facets() {
        let mut items = items();
        items[0].play_count = 1;
        items[2].play_count = 5;
        items[1].tags = vec!["fast".to_string()];
        let mut model = LibraryModel::new(items);
        model.set_sort(SortOrder::Played);
        let names: Vec<&str> = model.visible().iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Mike", "Zebra", "Alpha"]);
        assert!(model.available_facets().contains(&"fast".to_string()));
        model.set_category(Some("fast".to_string()));
        assert_eq!(model.visible_count(), 1);
        assert_eq!(
            model.selected_item().map(|i| i.name.as_str()),
            Some("Alpha")
        );
    }

    #[test]
    fn favorites_and_hidden_filters() {
        let mut items = items();
        items[0].favorite = true;
        items[1].hidden = true;
        let mut model = LibraryModel::new(items);
        assert_eq!(model.visible_count(), 2, "hidden excluded by default");
        model.set_favorites_only(true);
        assert_eq!(model.visible_count(), 1);
        assert_eq!(
            model.selected_item().map(|i| i.name.as_str()),
            Some("Zebra")
        );
        model.set_favorites_only(false);
        model.set_show_hidden(true);
        assert_eq!(model.visible_count(), 3);
    }

    #[test]
    fn category_chips_intersect() {
        let mut model = LibraryModel::new(items());
        assert_eq!(
            model.available_facets(),
            vec!["Audio", "Graphics", "Utility"]
        );
        model.set_category(Some("Graphics".to_string()));
        assert_eq!(model.visible_count(), 1);
        model.set_category(Some("Utility".to_string()));
        assert_eq!(model.visible_count(), 2);
        model.set_filter("mike");
        assert_eq!(model.visible_count(), 1);
        model.set_category(None);
        assert_eq!(model.visible_count(), 1);
    }

    #[test]
    fn ten_thousand_items_filter_fast() {
        let big: Vec<Item> = (0..10_000)
            .map(|i| {
                item(
                    &format!("/a/App{i:05}.AppImage"),
                    &format!("App{i:05}"),
                    "generated",
                    &["Utility"],
                    false,
                    i as u64,
                )
            })
            .collect();
        let mut model = LibraryModel::new(big);
        let start = std::time::Instant::now();
        model.set_filter("app042");
        let _ = model.visible_count();
        // Generous bound: asserts non-pathological scaling, not hardware speed.
        assert!(start.elapsed() < std::time::Duration::from_millis(1000));
    }
}
