//! Gives the Windows program its icon. The icon file's images are written as Windows resources into a COFF object of
//! our own making, which the linker takes like any other object, so no resource compiler (rc.exe or windres) is
//! needed, whether building on Windows or cross-building from Linux.

use std::path::PathBuf;

const ICON: &str = "assets/linewright.ico";
const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
const LANGUAGE: u32 = 0x0409; // English (United States), as resource compilers write it by default

fn main() {
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") || std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64")
    {
        return;
    }
    let ico = std::fs::read(ICON).expect("the icon file");
    let object = resource_object(&icon_resources(&ico));
    let path = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("linewright-icon.obj");
    std::fs::write(&path, object).expect("the icon object");
    println!("cargo:rustc-link-arg-bins={}", path.display());
}

/// One resource: its type, its id, and its bytes.
struct Resource {
    kind: u32,
    id: u32,
    data: Vec<u8>,
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// An .ico file as Windows keeps it in a program: each image an RT_ICON numbered from 1, and an RT_GROUP_ICON that
/// lists them, which is the icon Explorer shows.
fn icon_resources(ico: &[u8]) -> Vec<Resource> {
    let count = u16_at(ico, 4) as usize;
    let mut resources = vec![];
    let mut group = vec![0, 0, 1, 0];
    group.extend((count as u16).to_le_bytes());
    for i in 0..count {
        let entry = &ico[6 + 16 * i..6 + 16 * (i + 1)];
        let (size, offset) = (u32_at(entry, 8) as usize, u32_at(entry, 12) as usize);
        let id = i as u32 + 1;
        resources.push(Resource { kind: RT_ICON, id, data: ico[offset..offset + size].to_vec() });
        // The group entry is the file's entry with the image's place replaced by its resource id.
        group.extend(&entry[..12]);
        group.extend((id as u16).to_le_bytes());
    }
    resources.push(Resource { kind: RT_GROUP_ICON, id: 1, data: group });
    resources
}

/// A COFF object for x86-64 whose one section, .rsrc, holds the resources in the tree Windows reads: types, then ids,
/// then a language each, then the data. Data entries give their data's address relative to the image, so each has a
/// relocation against the section.
fn resource_object(resources: &[Resource]) -> Vec<u8> {
    let mut kinds: Vec<u32> = resources.iter().map(|r| r.kind).collect();
    kinds.sort_unstable();
    kinds.dedup();
    let directory = |entries: usize| 16 + 8 * entries;
    // Where each table goes: the root, a table per type, a table per resource for its language, then data entries.
    let root = 0;
    let mut at = directory(kinds.len());
    let type_tables: Vec<usize> = kinds
        .iter()
        .map(|kind| {
            let here = at;
            at += directory(resources.iter().filter(|r| r.kind == *kind).count());
            here
        })
        .collect();
    let ordered: Vec<&Resource> = kinds.iter().flat_map(|kind| resources.iter().filter(move |r| r.kind == *kind)).collect();
    let language_tables: Vec<usize> = ordered
        .iter()
        .map(|_| {
            let here = at;
            at += directory(1);
            here
        })
        .collect();
    let data_entries: Vec<usize> = ordered
        .iter()
        .map(|_| {
            let here = at;
            at += 16;
            here
        })
        .collect();
    let mut data_places = vec![];
    for resource in &ordered {
        at = at.next_multiple_of(8);
        data_places.push(at);
        at += resource.data.len();
    }
    let mut section = vec![0u8; at.next_multiple_of(8)];
    let put32 = |section: &mut Vec<u8>, at: usize, value: u32| section[at..at + 4].copy_from_slice(&value.to_le_bytes());
    let header = |section: &mut Vec<u8>, at: usize, ids: usize| section[at + 14..at + 16].copy_from_slice(&(ids as u16).to_le_bytes());
    const SUBDIRECTORY: u32 = 0x8000_0000;
    header(&mut section, root, kinds.len());
    for (k, kind) in kinds.iter().enumerate() {
        put32(&mut section, root + 16 + 8 * k, *kind);
        put32(&mut section, root + 20 + 8 * k, SUBDIRECTORY | type_tables[k] as u32);
    }
    let mut index = 0;
    for (k, kind) in kinds.iter().enumerate() {
        let of_kind: Vec<&Resource> = ordered.iter().copied().filter(|r| r.kind == *kind).collect();
        header(&mut section, type_tables[k], of_kind.len());
        for (j, resource) in of_kind.iter().enumerate() {
            put32(&mut section, type_tables[k] + 16 + 8 * j, resource.id);
            put32(&mut section, type_tables[k] + 20 + 8 * j, SUBDIRECTORY | language_tables[index] as u32);
            header(&mut section, language_tables[index], 1);
            put32(&mut section, language_tables[index] + 16, LANGUAGE);
            put32(&mut section, language_tables[index] + 20, data_entries[index] as u32);
            index += 1;
        }
    }
    let mut relocations = vec![];
    for (i, resource) in ordered.iter().enumerate() {
        // The address is written as the place in the section; the linker adds where the section lands.
        put32(&mut section, data_entries[i], data_places[i] as u32);
        put32(&mut section, data_entries[i] + 4, resource.data.len() as u32);
        section[data_places[i]..data_places[i] + resource.data.len()].copy_from_slice(&resource.data);
        relocations.push(data_entries[i] as u32);
    }

    const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
    const IMAGE_REL_AMD64_ADDR32NB: u16 = 3;
    const SECTION_FLAGS: u32 = 0x4000_0040; // initialized data, readable
    let section_start = 20 + 40;
    let relocations_start = section_start + section.len();
    let symbols_start = relocations_start + 10 * relocations.len();
    let mut out = vec![];
    out.extend(IMAGE_FILE_MACHINE_AMD64.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // sections
    out.extend(0u32.to_le_bytes()); // time stamp
    out.extend((symbols_start as u32).to_le_bytes());
    out.extend(2u32.to_le_bytes()); // the section's symbol and its auxiliary record
    out.extend(0u16.to_le_bytes()); // no optional header
    out.extend(0u16.to_le_bytes());
    out.extend(*b".rsrc\0\0\0");
    out.extend(0u32.to_le_bytes()); // virtual size
    out.extend(0u32.to_le_bytes()); // virtual address
    out.extend((section.len() as u32).to_le_bytes());
    out.extend((section_start as u32).to_le_bytes());
    out.extend((relocations_start as u32).to_le_bytes());
    out.extend(0u32.to_le_bytes()); // line numbers
    out.extend((relocations.len() as u16).to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(SECTION_FLAGS.to_le_bytes());
    out.extend(&section);
    for offset in &relocations {
        out.extend(offset.to_le_bytes());
        out.extend(0u32.to_le_bytes()); // symbol 0, the section
        out.extend(IMAGE_REL_AMD64_ADDR32NB.to_le_bytes());
    }
    // The section's symbol, static, with its auxiliary record giving the section's length and relocation count.
    out.extend(*b".rsrc\0\0\0");
    out.extend(0u32.to_le_bytes());
    out.extend(1i16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend([3u8, 1u8]);
    out.extend((section.len() as u32).to_le_bytes());
    out.extend((relocations.len() as u16).to_le_bytes());
    out.extend([0u8; 12]);
    out.extend(4u32.to_le_bytes()); // an empty string table
    out
}
