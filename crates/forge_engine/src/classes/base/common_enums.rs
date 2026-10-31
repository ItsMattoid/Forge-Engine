//! Roblox enums that belong to no single class: directions and surfaces, fonts, quality settings and the like.
//! (Scripts see them as `Enum.<Name>.<Item>`; items have the numbers Roblox gives them. Enums that go with one class
//! are declared in that class's file.)
//!
//! Some of these are only here so that scripts which mention them run: this engine has no GUI and no avatar
//! animation, so nothing reads `Enum.Font` or `Enum.AnimationPriority` yet.

use crate::core::EnumDesc;

pub static ENUMS: &[EnumDesc] = &[
    EnumDesc::numbered("Axis", &["X", "Y", "Z"], &[0, 1, 2]),
    EnumDesc::numbered("NormalId", &["Right", "Top", "Back", "Left", "Bottom", "Front"], &[0, 1, 2, 3, 4, 5]),
    EnumDesc::numbered(
        "SurfaceType",
        &["Smooth", "Glue", "Weld", "Studs", "Inlet", "Universal", "Hinge", "Motor", "SteppingMotor", "SmoothNoOutlines"],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 10],
    ),
    EnumDesc::new(
        "Font",
        &[
            "Legacy", "Arial", "ArialBold", "SourceSans", "SourceSansBold", "SourceSansSemibold", "SourceSansLight", "SourceSansItalic", "Bodoni", "Garamond", "Cartoon", "Code", "Highway", "SciFi", "Arcade",
            "Fantasy", "Antique", "Gotham", "GothamSemibold", "GothamBold", "GothamBlack", "AmaticSC", "Bangers", "Creepster", "DenkOne", "Fondamento", "FredokaOne", "GrenzeGotisch", "IndieFlower", "JosefinSans",
            "Jura", "Kalam", "LuckiestGuy", "Merriweather", "Michroma", "Nunito", "Oswald", "PatrickHand", "PermanentMarker", "Roboto", "RobotoCondensed", "RobotoMono", "Sarpanch", "SpecialElite", "TitilliumWeb",
            "Ubuntu", "Unknown",
        ],
    ),
    EnumDesc::numbered("FontWeight", &["Thin", "ExtraLight", "Light", "Regular", "Medium", "SemiBold", "Bold", "ExtraBold", "Heavy"], &[100, 200, 300, 400, 500, 600, 700, 800, 900]),
    EnumDesc::numbered("FontStyle", &["Normal", "Italic"], &[0, 1]),
    EnumDesc::new(
        "QualityLevel",
        &[
            "Automatic", "Level01", "Level02", "Level03", "Level04", "Level05", "Level06", "Level07", "Level08", "Level09", "Level10", "Level11", "Level12", "Level13", "Level14", "Level15", "Level16", "Level17",
            "Level18", "Level19", "Level20", "Level21",
        ],
    ),
    EnumDesc::new("SavedQualityLevel", &["Automatic", "QualityLevel1", "QualityLevel2", "QualityLevel3", "QualityLevel4", "QualityLevel5", "QualityLevel6", "QualityLevel7", "QualityLevel8", "QualityLevel9", "QualityLevel10"]),
    EnumDesc::new("MeshPartDetailLevel", &["DistanceBased", "Level01", "Level02", "Level03", "Level04"]),
    EnumDesc::new("ControlMode", &["MouseLockSwitch", "Classic"]),
    EnumDesc::new("FormFactor", &["Symmetric", "Brick", "Plate", "Custom"]),
    EnumDesc::new("CollisionFidelity", &["Default", "Hull", "Box", "PreciseConvexDecomposition"]),
    EnumDesc::new("RenderFidelity", &["Automatic", "Precise", "Performance"]),
    EnumDesc::new("DominantAxis", &["Width", "Height"]),
    EnumDesc::new("Limb", &["Head", "Torso", "LeftArm", "RightArm", "LeftLeg", "RightLeg", "Unknown"]),
    EnumDesc::new("AccessoryType", &["Unknown", "Hat", "Hair", "Face", "Neck", "Shoulder", "Front", "Back", "Waist", "TShirt", "Shirt", "Pants", "Jacket", "Sweater", "Shorts", "LeftShoe", "RightShoe", "DressSkirt"]),
    EnumDesc::numbered("AnimationPriority", &["Idle", "Movement", "Action", "Action2", "Action3", "Action4", "Core"], &[0, 1, 2, 3, 4, 5, 1000]),
    EnumDesc::new("RollOffMode", &["Inverse", "Linear", "LinearSquare", "InverseTapered"]),
    EnumDesc::numbered("MessageType", &["MessageOutput", "MessageInfo", "MessageWarning", "MessageError"], &[0, 1, 2, 3]),
    EnumDesc::new("AlignType", &["Parallel", "Perpendicular", "PrimaryAxisParallel", "PrimaryAxisPerpendicular", "PrimaryAxisLookAt", "AllAxes"]),
    EnumDesc::new("ActuatorType", &["None", "Motor", "Servo"]),
    EnumDesc::numbered("Platform", &["Windows", "OSX", "IOS", "Android", "Linux", "None"], &[0, 1, 2, 3, 13, 19]),
];
