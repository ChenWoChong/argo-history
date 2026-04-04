use askama::Template;

#[derive(Debug, Clone)]
pub struct SidebarObject {
    pub name: String,
    pub namespace: String,
    pub latest_timestamp: String,
    pub version_count: usize,
    pub href: String,
    pub is_selected: bool,
}

#[derive(Debug, Clone)]
pub struct VersionLink {
    pub title: String,
    pub timestamp: String,
    pub href: String,
    pub download_href: String,
    pub is_active: bool,
}

#[derive(Template)]
#[template(path = "history.html")]
pub struct HistoryTemplate {
    pub active_route: &'static str,
    pub app_count: usize,
    pub appset_count: usize,
    pub objects: Vec<SidebarObject>,
    pub has_selection: bool,
    pub selected_name: String,
    pub selected_namespace: String,
    pub versions: Vec<VersionLink>,
    pub preview_content: String,
}
