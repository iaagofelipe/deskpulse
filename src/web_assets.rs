pub fn serve_web_asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    if path.contains("..") {
        return None;
    }
    match path {
        "css/tokens.css" => Some(("text/css; charset=utf-8", include_bytes!("../web/css/tokens.css"))),
        "css/layout.css" => Some(("text/css; charset=utf-8", include_bytes!("../web/css/layout.css"))),
        "css/views.css" => Some(("text/css; charset=utf-8", include_bytes!("../web/css/views.css"))),
        "css/admin.css" => Some(("text/css; charset=utf-8", include_bytes!("../web/css/admin.css"))),
        "js/icons.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/icons.js"))),
        "js/charts.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/charts.js"))),
        "js/api.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/api.js"))),
        "js/app.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/app.js"))),
        "js/admin.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/admin.js"))),
        "js/views/home.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/views/home.js"))),
        "js/views/pc.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/views/pc.js"))),
        "js/views/ai.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/views/ai.js"))),
        "js/views/agenda.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/views/agenda.js"))),
        "js/views/weather.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/views/weather.js"))),
        "js/views/media.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/views/media.js"))),
        "js/views/github.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/views/github.js"))),
        "js/views/shortcuts.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/views/shortcuts.js"))),
        "js/views/timer.js" => Some(("application/javascript; charset=utf-8", include_bytes!("../web/js/views/timer.js"))),
        _ => None,
    }
}
