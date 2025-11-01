use image::DynamicImage;
use once_cell::sync::Lazy;

pub static ICON_ATC: Lazy<DynamicImage> = Lazy::new(|| {
    let data = include_bytes!("../assets/icon/classic/ATC.png");
    image::load_from_memory(data).expect("Failed to load ATC.png")
});

pub static ICON_BTN: Lazy<DynamicImage> = Lazy::new(|| {
    let data = include_bytes!("../assets/icon/classic/BTN.png");
    image::load_from_memory(data).expect("Failed to load BTN.png")
});

pub static ICON_DISATC: Lazy<DynamicImage> = Lazy::new(|| {
    let data = include_bytes!("../assets/icon/classic/DISATC.png");
    image::load_from_memory(data).expect("Failed to load DISATC.png")
});

pub static ICON_DISBTN: Lazy<DynamicImage> = Lazy::new(|| {
    let data = include_bytes!("../assets/icon/classic/DISBTN.png");
    image::load_from_memory(data).expect("Failed to load DISBTN.png")
});

pub static ICON_DISPAS: Lazy<DynamicImage> = Lazy::new(|| {
    let data = include_bytes!("../assets/icon/classic/DISPAS.png");
    image::load_from_memory(data).expect("Failed to load DISPAS.png")
});

pub static ICON_PAS: Lazy<DynamicImage> = Lazy::new(|| {
    let data = include_bytes!("../assets/icon/classic/PAS.png");
    image::load_from_memory(data).expect("Failed to load PAS.png")
});