use askama::Template;

#[derive(Debug, Clone)]
pub struct SidebarObject {
    pub name: String,
    pub namespace: String,
    pub source_label: String,
    pub latest_timestamp: String,
    pub version_count: usize,
    pub href: String,
    pub is_selected: bool,
}

#[derive(Debug, Clone)]
pub struct SidebarGroup {
    pub title: String,
    pub count: usize,
    pub open: bool,
    pub objects: Vec<SidebarObject>,
}

#[derive(Debug, Clone)]
pub struct PaginationLink {
    pub label: String,
    pub href: String,
    pub is_active: bool,
}

#[derive(Debug, Clone)]
pub struct Pagination {
    pub links: Vec<PaginationLink>,
    pub input_name: String,
    pub form_action: String,
    pub total_items: usize,
    pub total_pages: usize,
}

#[derive(Debug, Clone)]
pub struct VersionLink {
    pub title: String,
    pub timestamp: String,
    pub href: String,
    pub download_href: String,
    pub is_active: bool,
}

#[derive(Debug, Clone)]
pub struct CodeToken {
    pub class_name: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct CodeLine {
    pub class_name: String,
    pub prefix: String,
    pub tokens: Vec<CodeToken>,
}

#[derive(Debug, Clone)]
pub struct HighlightedBlock {
    pub title: String,
    pub lines: Vec<CodeLine>,
}

#[derive(Template)]
#[template(path = "history.html")]
pub struct HistoryTemplate {
    pub active_route: &'static str,
    pub apps_href: String,
    pub appsets_href: String,
    pub search_action: String,
    pub app_count: usize,
    pub appset_count: usize,
    pub object_groups: Vec<SidebarGroup>,
    pub object_pagination: Option<Pagination>,
    pub current_objects_page: usize,
    pub search_query: String,
    pub has_selection: bool,
    pub selected_name: String,
    pub selected_namespace: String,
    pub selected_source_label: String,
    pub versions: Vec<VersionLink>,
    pub versions_pagination: Option<Pagination>,
    pub preview_block: HighlightedBlock,
    pub diff_block: Option<HighlightedBlock>,
}
