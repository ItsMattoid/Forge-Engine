//! The `BrickColor` palette: Roblox's named colours. A part's `BrickColor` is its `Color` snapped to the nearest
//! entry; setting it sets the colour. (The list holds the colours Roblox scripts use; the RGB values are the
//! ones Roblox publishes for them.)

use bevy::math::Vec3;

pub struct BrickColorDef {
    pub number: u16,
    pub name: &'static str,
    pub rgb: [u8; 3],
}

impl BrickColorDef {
    pub fn color(&self) -> Vec3 {
        Vec3::new(self.rgb[0] as f32, self.rgb[1] as f32, self.rgb[2] as f32) / 255.0
    }
}

macro_rules! palette {
    ($(($n:expr, $name:expr, $r:expr, $g:expr, $b:expr)),* $(,)?) => {
        &[$(BrickColorDef { number: $n, name: $name, rgb: [$r, $g, $b] }),*]
    };
}

/// "Medium stone grey", the colour new parts have.
pub const DEFAULT_NUMBER: u16 = 194;

#[rustfmt::skip]
pub static PALETTE: &[BrickColorDef] = palette![
    (1, "White", 242, 243, 243), (2, "Grey", 161, 165, 162), (3, "Light yellow", 249, 233, 153), (5, "Brick yellow", 215, 197, 154),
    (6, "Light green (Mint)", 194, 218, 184), (9, "Light reddish violet", 232, 186, 200), (11, "Pastel Blue", 128, 187, 219),
    (12, "Light orange brown", 203, 132, 66), (18, "Nougat", 204, 142, 105), (21, "Bright red", 196, 40, 28),
    (22, "Med. reddish violet", 196, 112, 160), (23, "Bright blue", 13, 105, 172), (24, "Bright yellow", 245, 205, 48),
    (25, "Earth orange", 98, 71, 50), (26, "Black", 27, 42, 53), (27, "Dark grey", 109, 110, 110), (28, "Dark green", 40, 127, 71),
    (29, "Medium green", 161, 196, 140), (36, "Lig. Yellowich orange", 243, 207, 155), (37, "Bright green", 75, 151, 75),
    (38, "Dark orange", 160, 95, 53), (39, "Light bluish violet", 193, 202, 222), (40, "Transparent", 236, 236, 236),
    (41, "Tr. Red", 205, 84, 75), (42, "Tr. Lg blue", 193, 223, 240), (43, "Tr. Blue", 123, 182, 232), (44, "Tr. Yellow", 247, 241, 141),
    (45, "Light blue", 180, 210, 228), (47, "Tr. Flu. Reddish orange", 217, 133, 108), (48, "Tr. Green", 132, 182, 141),
    (49, "Tr. Flu. Green", 248, 241, 132), (50, "Phosph. White", 236, 232, 222), (100, "Light red", 238, 196, 182),
    (101, "Medium red", 218, 134, 122), (102, "Medium blue", 110, 153, 202), (103, "Light grey", 199, 193, 183),
    (104, "Bright violet", 107, 50, 124), (105, "Br. yellowish orange", 226, 155, 64), (106, "Bright orange", 218, 133, 65),
    (107, "Bright bluish green", 0, 143, 156), (108, "Earth yellow", 104, 92, 67), (110, "Bright bluish violet", 67, 84, 147),
    (111, "Tr. Brown", 191, 183, 177), (112, "Medium bluish violet", 104, 116, 172), (113, "Tr. Medi. reddish violet", 229, 173, 200),
    (115, "Med. yellowish green", 199, 210, 60), (116, "Med. bluish green", 85, 165, 175), (118, "Light bluish green", 183, 215, 213),
    (119, "Br. yellowish green", 164, 189, 71), (120, "Lig. yellowish green", 217, 228, 167), (121, "Med. yellowish orange", 231, 172, 88),
    (123, "Br. reddish orange", 211, 111, 76), (124, "Bright reddish violet", 146, 57, 120), (125, "Light orange", 234, 184, 146),
    (126, "Tr. Bright bluish violet", 165, 165, 203), (127, "Gold", 220, 188, 129), (128, "Dark nougat", 174, 122, 89),
    (131, "Silver", 156, 163, 168), (133, "Neon orange", 213, 115, 61), (134, "Neon green", 216, 221, 86), (135, "Sand blue", 116, 134, 157),
    (136, "Sand violet", 135, 124, 144), (137, "Medium orange", 224, 152, 100), (138, "Sand yellow", 149, 138, 115),
    (140, "Earth blue", 32, 58, 86), (141, "Earth green", 39, 70, 45), (143, "Tr. Flu. Blue", 207, 226, 247), (145, "Sand blue metallic", 121, 136, 161),
    (146, "Sand violet metallic", 149, 142, 163), (147, "Sand yellow metallic", 147, 135, 103), (148, "Dark grey metallic", 87, 88, 87),
    (149, "Black metallic", 22, 29, 50), (150, "Light grey metallic", 171, 173, 172), (151, "Sand green", 120, 144, 130),
    (153, "Sand red", 149, 121, 119), (154, "Dark red", 123, 46, 47), (157, "Tr. Flu. Yellow", 255, 246, 123), (158, "Tr. Flu. Red", 225, 164, 194),
    (168, "Gun metallic", 117, 108, 98), (176, "Red flip/flop", 151, 105, 91), (178, "Yellow flip/flop", 180, 132, 85),
    (179, "Silver flip/flop", 137, 135, 136), (180, "Curry", 215, 169, 75), (190, "Fire Yellow", 249, 214, 46),
    (191, "Flame yellowish orange", 232, 171, 45), (192, "Reddish brown", 105, 64, 40), (193, "Flame reddish orange", 207, 96, 36),
    (194, "Medium stone grey", 163, 162, 165), (195, "Royal blue", 70, 103, 164), (196, "Dark Royal blue", 35, 71, 139),
    (198, "Bright reddish lilac", 142, 66, 133), (199, "Dark stone grey", 99, 95, 98), (200, "Lemon metalic", 130, 138, 93),
    (208, "Light stone grey", 229, 228, 223), (209, "Dark Curry", 176, 142, 68), (210, "Faded green", 112, 149, 120), (211, "Turquoise", 121, 181, 181),
    (212, "Light Royal blue", 159, 195, 233), (213, "Medium Royal blue", 108, 129, 183), (216, "Rust", 144, 76, 42), (217, "Brown", 124, 92, 70),
    (218, "Reddish lilac", 150, 112, 159), (219, "Lilac", 107, 98, 155), (220, "Light lilac", 167, 169, 206), (221, "Bright purple", 205, 98, 152),
    (222, "Light purple", 228, 173, 200), (223, "Light pink", 220, 144, 149), (224, "Light brick yellow", 240, 213, 160),
    (225, "Warm yellowish orange", 235, 184, 127), (226, "Cool yellow", 253, 234, 141), (232, "Dove blue", 125, 187, 221),
    (268, "Medium lilac", 52, 43, 117), (301, "Slime green", 80, 109, 84), (302, "Smoky grey", 91, 93, 105), (303, "Dark blue", 0, 16, 176),
    (304, "Parsley green", 44, 101, 29), (305, "Steel blue", 82, 124, 174), (306, "Storm blue", 51, 88, 130), (307, "Lapis", 16, 42, 220),
    (308, "Dark indigo", 61, 21, 133), (309, "Sea green", 52, 142, 64), (310, "Shamrock", 91, 154, 76), (311, "Fossil", 159, 161, 172),
    (312, "Mulberry", 89, 34, 89), (313, "Forest green", 31, 128, 29), (314, "Cadet blue", 159, 173, 192), (315, "Electric blue", 9, 137, 207),
    (316, "Eggplant", 123, 0, 123), (317, "Moss", 124, 156, 107), (318, "Artichoke", 138, 171, 133), (319, "Sage green", 185, 196, 177),
    (320, "Ghost grey", 202, 203, 209), (321, "Lilac", 167, 94, 155), (322, "Plum", 123, 47, 123), (323, "Pastel green", 148, 190, 129),
    (1001, "Institutional white", 248, 248, 248), (1002, "Mid gray", 205, 205, 205), (1003, "Really black", 17, 17, 17),
    (1004, "Really red", 255, 0, 0), (1005, "Deep orange", 255, 176, 0), (1006, "Alder", 180, 128, 255), (1007, "Dusty Rose", 163, 75, 75),
    (1008, "Olive", 193, 190, 66), (1009, "New Yeller", 255, 255, 0), (1010, "Really blue", 0, 0, 255), (1011, "Navy blue", 0, 32, 96),
    (1012, "Deep blue", 33, 84, 185), (1013, "Cyan", 4, 175, 236), (1014, "CGA brown", 170, 85, 0), (1015, "Magenta", 170, 0, 170),
    (1016, "Pink", 255, 102, 204), (1017, "Deep orange", 255, 175, 0), (1018, "Teal", 18, 238, 212), (1019, "Toothpaste", 0, 255, 255),
    (1020, "Lime green", 0, 255, 0), (1021, "Camo", 58, 125, 21), (1022, "Grime", 127, 142, 100), (1023, "Lavender", 140, 91, 159),
    (1024, "Pastel light blue", 175, 221, 255), (1025, "Pastel orange", 255, 201, 201), (1026, "Pastel violet", 177, 167, 255),
    (1027, "Pastel blue-green", 159, 243, 233), (1028, "Pastel green", 204, 255, 204), (1029, "Pastel yellow", 255, 255, 204),
    (1030, "Pastel brown", 255, 204, 153), (1031, "Royal purple", 98, 37, 209), (1032, "Hot pink", 255, 0, 191),
];

pub fn by_number(number: u16) -> Option<&'static BrickColorDef> {
    PALETTE.iter().find(|c| c.number == number)
}

pub fn by_name(name: &str) -> Option<&'static BrickColorDef> {
    PALETTE.iter().find(|c| c.name.eq_ignore_ascii_case(name))
}

/// The palette colour closest to `color` (in RGB), as `BrickColor.new(r, g, b)` and a part's `BrickColor` find it.
pub fn nearest(color: Vec3) -> &'static BrickColorDef {
    PALETTE
        .iter()
        .min_by(|a, b| a.color().distance_squared(color).total_cmp(&b.color().distance_squared(color)))
        .expect("the palette is not empty")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_unique_and_colours_snap_to_the_nearest_entry() {
        let mut numbers: Vec<u16> = PALETTE.iter().map(|c| c.number).collect();
        numbers.sort_unstable();
        let total = numbers.len();
        numbers.dedup();
        assert_eq!(numbers.len(), total, "no number is listed twice");
        assert_eq!(by_number(21).unwrap().name, "Bright red");
        assert_eq!(by_name("bright red").unwrap().number, 21);
        assert_eq!(nearest(Vec3::new(1.0, 0.0, 0.0)).name, "Really red");
        assert_eq!(nearest(by_number(DEFAULT_NUMBER).unwrap().color()).number, DEFAULT_NUMBER);
        assert_eq!(nearest(Vec3::new(0.64, 0.64, 0.64)).number, DEFAULT_NUMBER, "a new part's colour is Medium stone grey");
    }
}
