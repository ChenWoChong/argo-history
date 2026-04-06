use askama::Template;

#[derive(Debug, Clone)]
pub struct FilterChip {
    pub label: String,
    pub href: String,
    pub active: bool,
}

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
    pub href: Option<String>,
    pub is_active: bool,
    pub is_gap: bool,
}

#[derive(Debug, Clone)]
pub struct Pagination {
    pub links: Vec<PaginationLink>,
    pub input_name: String,
    pub form_action: String,
    pub total_items: usize,
    pub total_pages: usize,
    pub input_placeholder: String,
}

#[derive(Debug, Clone)]
pub struct VersionLink {
    pub title: String,
    pub timestamp: String,
    pub operation: String,
    pub summary: String,
    pub href: String,
    pub download_href: String,
    pub is_active: bool,
}

#[derive(Debug, Clone)]
pub struct TimelineGroup {
    pub label: String,
    pub versions: Vec<VersionLink>,
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
    pub operation_filters: Vec<FilterChip>,
    pub time_filters: Vec<FilterChip>,
    pub retention_days: u64,
    pub has_selection: bool,
    pub selected_name: String,
    pub selected_namespace: String,
    pub selected_source_label: String,
    pub overview_first_backup_at: String,
    pub overview_latest_backup_at: String,
    pub overview_total_versions: usize,
    pub overview_latest_operation: String,
    pub timeline_groups: Vec<TimelineGroup>,
    pub versions_pagination: Option<Pagination>,
    pub preview_block: HighlightedBlock,
    pub diff_block: Option<HighlightedBlock>,
}
