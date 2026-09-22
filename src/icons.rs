pub fn register(display: &gtk::gdk::Display) {
    gtk::gio::resources_register_include!("bearbar.gresource")
        .expect("Bearbar icon resources are valid");
    gtk::IconTheme::for_display(display).add_resource_path("/dev/bearbar/icons");
}

pub fn image(name: &str, size: i32) -> gtk::Image {
    let image = gtk::Image::from_icon_name(name);
    image.set_pixel_size(size);
    image
}
