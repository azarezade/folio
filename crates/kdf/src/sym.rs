//! YJ symbol ids used by the container code, and kfxlib's fragment tables.
//!
//! A YJ shared symbol's id is the number in its kfxlib name (`$260` is id 260).
//! Names follow kfxlib's usage where it is clear; the tables are copied from
//! `kfxlib/yj_container.py` and `yj_structure.py` and keep its `$N` numbering.

use folio_ion::SymbolId;

pub const ION_SYMBOL_TABLE: SymbolId = 3;
pub const NAME: SymbolId = 4;

pub const LANGUAGE: SymbolId = 10;
pub const STORY_CONTENT: SymbolId = 146;
pub const FORMAT: SymbolId = 161;
pub const CONTENT_TYPE: SymbolId = 159;
pub const READING_ORDERS: SymbolId = 169;
pub const SECTIONS: SymbolId = 170;
pub const SECTION_NAME: SymbolId = 174;
pub const STORY_NAME: SymbolId = 176;
pub const READING_ORDER_NAME: SymbolId = 178;
pub const PAGE_TEMPLATES: SymbolId = 141;
pub const METADATA: SymbolId = 258;
pub const STORY: SymbolId = 259;
pub const SECTION: SymbolId = 260;
pub const CONTAINER_CONTENT: SymbolId = 270;
/// The container-info fragment kfxlib adds (same symbol as container content).
pub const CONTAINER: SymbolId = 270;
pub const KEY: SymbolId = 492;
pub const VALUE: SymbolId = 307;
pub const RAW_MEDIA: SymbolId = 417;
pub const COVER_IMAGE: SymbolId = 424;
pub const BOOK_METADATA: SymbolId = 490;
pub const CATEGORISED_METADATA: SymbolId = 491;
pub const CATEGORY: SymbolId = 495;
pub const DOCUMENT_DATA: SymbolId = 538;
pub const APPLICATION_VERSION: SymbolId = 587;
pub const PACKAGE_VERSION: SymbolId = 588;
pub const CAPABILITIES: SymbolId = 593;
pub const AUXILIARY_DATA: SymbolId = 597;
pub const KFX_ID: SymbolId = 598;
pub const STRUCTURE: SymbolId = 608;

pub const ROOT_FRAGMENT_TYPES: [SymbolId; 17] = [
    3, 270, 490, 389, 419, 585, 538, 262, 593, 550, 258, 265, 264, 395, 390, 621, 611,
];

pub const REQUIRED_BOOK_FRAGMENT_TYPES: [SymbolId; 11] = [3, 270, 490, 389, 419, 538, 550, 258, 265, 264, 611];

pub const ALLOWED_BOOK_FRAGMENT_TYPES: [SymbolId; 24] = [
    266, 597, 418, 417, 394, 145, 585, 610, 164, 262, 593, 391, 692, 387, 395, 756, 260, 267, 390, 609, 259, 608, 157,
    621,
];

pub const CONTAINER_FRAGMENT_TYPES: [SymbolId; 4] = [270, 593, 3, 419];

pub fn is_known_fragment_type(t: SymbolId) -> bool {
    REQUIRED_BOOK_FRAGMENT_TYPES.contains(&t) || ALLOWED_BOOK_FRAGMENT_TYPES.contains(&t)
}

/// Struct keys that hold a fragment's own id, per fragment type (`FRAGMENT_ID_KEYS`).
pub fn fragment_id_keys(ftype: SymbolId) -> &'static [SymbolId] {
    match ftype {
        266 => &[180],
        597 => &[174, 598],
        418 | 417 => &[165],
        394 => &[240],
        145 | 692 => &[NAME],
        164 => &[175],
        391 => &[239],
        387 | 260 | 267 | 609 => &[174],
        756 => &[757],
        259 => &[176],
        608 => &[598],
        157 => &[173],
        610 => &[602],
        _ => &[],
    }
}

/// Fragment type referenced by a symbol under struct key `key` (`COMMON_FRAGMENT_REFERENCES`).
pub fn common_reference(key: SymbolId) -> Option<SymbolId> {
    Some(match key {
        749 => 259,
        266 => 266,
        597 => 597,
        429 => 157,
        479 => 164,
        145 => 145,
        146 => 608,
        245 => 164,
        179 => 266,
        165 => 417,
        392 => 391,
        NAME => 145,
        167 => 164,
        175 => 164,
        757 => 756,
        174 => 260,
        170 => 260,
        176 => 259,
        157 => 157,
        173 => 157,
        528 => 164,
        214 => 164,
        636 => 417,
        635 => 164,
        _ => return None,
    })
}

/// `NESTED_FRAGMENT_REFERENCES`, keyed by (parent key, key).
pub fn nested_reference(parent: SymbolId, key: SymbolId) -> Option<SymbolId> {
    match (parent, key) {
        (597, 351) | (597, 538) | (597, 613) | (597, 614) => Some(597),
        _ => None,
    }
}

/// `SPECIAL_FRAGMENT_REFERENCES`, keyed by (fragment type, key).
pub fn special_reference(ftype: SymbolId, key: SymbolId) -> Option<SymbolId> {
    match (ftype, key) {
        (391, 247) => Some(394),
        (387, 212..=214) => Some(164),
        _ => None,
    }
}

/// Struct keys whose values are EID references (`EID_REFERENCES`).
pub const EID_REFERENCES: [SymbolId; 6] = [185, 155, 598, 754, 474, 163];

/// Fragment types whose `$155`/`$598` values are not EID definitions.
pub const NON_EID_DEFINING_TYPES: [SymbolId; 7] = [550, 265, 264, 609, 610, 621, 611];
