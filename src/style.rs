use gtk::gdk;

pub(crate) fn load_default_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("../style.css"));

    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
