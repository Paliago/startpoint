use rune::Icon;

#[cfg(target_os = "windows")]
pub static MINIMIZE: Icon = Icon {
    name: "window-minimize",
    view: 16.0,
    body: r#"<path d="M3 8.5h10" fill="none" stroke="currentColor"/>"#,
};
#[cfg(target_os = "windows")]
pub static MAXIMIZE: Icon = Icon {
    name: "window-maximize",
    view: 16.0,
    body: r#"<path d="M3.5 3.5h9v9h-9z" fill="none" stroke="currentColor"/>"#,
};
#[cfg(target_os = "windows")]
pub static RESTORE: Icon = Icon {
    name: "window-restore",
    view: 16.0,
    body: r#"<path d="M5.5 5.5v-2h7v7h-2m-7-5h7v7h-7z" fill="none" stroke="currentColor" stroke-linejoin="miter"/>"#,
};
#[cfg(target_os = "windows")]
pub static CLOSE: Icon = Icon {
    name: "window-close",
    view: 16.0,
    body: r#"<path d="m4 4 8 8m0-8-8 8" fill="none" stroke="currentColor"/>"#,
};

pub static PEN: Icon = Icon {
    name: "startpoint-pen",
    view: 24.0,
    body: r#"<path d="m15 4 5 5-11 11H4v-5zM13 6l5 5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>"#,
};
pub static TRASH: Icon = Icon {
    name: "startpoint-trash",
    view: 24.0,
    body: r#"<path d="M4 6h16M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7M14 10v7" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>"#,
};

pub static BACK: Icon = Icon {
    name: "startpoint-back",
    view: 24.0,
    body: r#"<path d="M19 12H5m6-6-6 6 6 6" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
pub static FORWARD: Icon = Icon {
    name: "startpoint-forward",
    view: 24.0,
    body: r#"<path d="M5 12h14m-6-6 6 6-6 6" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
pub static GRID: Icon = Icon {
    name: "startpoint-grid",
    view: 24.0,
    body: r#"<path d="M4 4h6v7H4zM14 4h6v7h-6zM4 15h6v5H4zM14 15h6v5h-6z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/>"#,
};
pub static TILES: Icon = Icon {
    name: "startpoint-tiles",
    view: 24.0,
    body: r#"<path d="M4 4h16v6H4zM4 14h16v6H4z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/>"#,
};
pub static LIST: Icon = Icon {
    name: "startpoint-list",
    view: 24.0,
    body: r#"<path d="M9 6h11M9 12h11M9 18h11M4 6h.1M4 12h.1M4 18h.1" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>"#,
};
pub static SORT: Icon = Icon {
    name: "startpoint-sort",
    view: 24.0,
    body: r#"<path d="M8 5v14m-4-4 4 4 4-4M16 19V5m-4 4 4-4 4 4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

pub static SIDEBAR: Icon = Icon {
    name: "startpoint-sidebar",
    view: 24.0,
    body: r#"<rect x="3" y="4" width="18" height="16" rx="3" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M9 4v16" stroke="currentColor" stroke-width="1.6"/>"#,
};
pub static SETTINGS: Icon = Icon {
    name: "startpoint-settings",
    view: 24.0,
    body: r#"<path d="M12 3l2 3 3-.3.5 3 2.5 2-2 2.5.3 3-3 .5-2 2.5-2.5-2-3 .3-.5-3L4 12l2-2.5-.3-3 3-.5z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><circle cx="12" cy="11.8" r="3" fill="none" stroke="currentColor" stroke-width="1.5"/>"#,
};
pub static CUSTOMIZE: Icon = Icon {
    name: "startpoint-customize",
    view: 24.0,
    body: r#"<path d="M5 5v14M12 5v14M19 5v14" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><path d="M3 9h4M10 15h4M17 8h4" stroke="currentColor" stroke-width="3.5" stroke-linecap="round"/>"#,
};
pub static REFRESH: Icon = Icon {
    name: "startpoint-refresh",
    view: 24.0,
    body: r#"<path d="M20 10a8 8 0 10-1.5 7M20 4v6h-6" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
pub static ADD: Icon = Icon {
    name: "startpoint-add",
    view: 24.0,
    body: r#"<path d="M12 5v14M5 12h14" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>"#,
};
pub static NOTIFICATIONS: Icon = Icon {
    name: "startpoint-notifications",
    view: 24.0,
    body: r#"<path d="M5 17h14l-2-3V9a5 5 0 00-10 0v5zM10 20h4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
pub static PLAY: Icon = Icon {
    name: "startpoint-play",
    view: 24.0,
    body: r#"<path d="M8 5l11 7-11 7z" fill="currentColor"/>"#,
};

pub static FAVOURITE: Icon = Icon {
    name: "startpoint-favourite",
    view: 24.0,
    body: r#"<path d="m12 3 2.8 5.8 6.4.9-4.6 4.5 1.1 6.3-5.7-3-5.7 3 1.1-6.3L2.8 9.7l6.4-.9z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>"#,
};

pub static FOLDER: Icon = Icon {
    name: "startpoint-folder",
    view: 24.0,
    body: r#"<path d="M3 7V5h6l2 3h10v12H3V7z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>"#,
};
