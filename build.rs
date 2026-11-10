//! Puts res/onion-peel.ico into onion-peel.exe as its icon, plus a version resource (the name,
//! publisher and version Windows shows in Task Manager, Startup apps and the file's Properties),
//! with no resource compiler: both are written as a COFF object holding a `.rsrc` section, which
//! the linker takes as it is.

use std::path::Path;

const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
const RT_VERSION: u32 = 16;
const LANG_EN_US: u32 = 0x409;

fn main() {
    println!("cargo:rerun-if-changed=res/onion-peel.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");
    let ico = std::fs::read("res/onion-peel.ico").expect("res/onion-peel.ico");
    let obj = Path::new(&std::env::var("OUT_DIR").unwrap()).join("icon.o");
    std::fs::write(&obj, coff(&rsrc(&ico))).unwrap();
    println!("cargo:rustc-link-arg-bins={}", obj.display());
}

fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn pad4(v: &mut Vec<u8>) {
    v.resize(v.len().next_multiple_of(4), 0);
}

fn wide_z(v: &mut Vec<u8>, s: &str) {
    for c in s.encode_utf16().chain([0]) {
        put16(v, c);
    }
}

/// One block of a version resource: length, value length, type (1 = text), key, value, children.
/// `value_len` counts WORDs for text values and bytes otherwise.
fn block(key: &str, value: &[u8], value_len: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut v = vec![0, 0];
    put16(&mut v, value_len);
    put16(&mut v, text as u16);
    wide_z(&mut v, key);
    pad4(&mut v);
    v.extend_from_slice(value);
    for c in children {
        pad4(&mut v);
        v.extend_from_slice(c);
    }
    let n = v.len() as u16;
    v[..2].copy_from_slice(&n.to_le_bytes());
    v
}

/// VS_VERSIONINFO for this build, from Cargo.toml's version.
fn version() -> Vec<u8> {
    let part = |k: &str| std::env::var(k).unwrap().parse::<u16>().unwrap() as u32;
    let (major, minor, patch) = (part("CARGO_PKG_VERSION_MAJOR"), part("CARGO_PKG_VERSION_MINOR"), part("CARGO_PKG_VERSION_PATCH"));
    let (ms, ls) = (major << 16 | minor, patch << 16);
    let mut fixed = vec![];
    for x in [0xFEEF_04BD, 0x0001_0000, ms, ls, ms, ls, 0x3F, 0, 0x0004_0004, 1, 0, 0, 0] {
        put32(&mut fixed, x); // signature, struct version, file and product versions, flags mask,
                              // flags, OS (NT, Win32), type (app), subtype, date
    }
    let dotted = format!("{major}.{minor}.{patch}.0");
    let pkg = std::env::var("CARGO_PKG_VERSION").unwrap();
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", "OnionAlien"),
        ("FileDescription", "Onion Peel"),
        ("FileVersion", &dotted),
        ("InternalName", "onion-peel"),
        ("LegalCopyright", "OnionAlien, GPL-3.0"),
        ("OriginalFilename", "onion-peel.exe"),
        ("ProductName", "Onion Peel"),
        ("ProductVersion", &pkg),
    ]
    .iter()
    .map(|(k, s)| {
        let mut val = vec![];
        wide_z(&mut val, s);
        block(k, &val, s.encode_utf16().count() as u16 + 1, true, &[])
    })
    .collect();
    let table = block("040904B0", &[], 0, true, &strings); // US English, Unicode
    let string_info = block("StringFileInfo", &[], 0, true, &[table]);
    let translation = block("Translation", &[0x09, 0x04, 0xB0, 0x04], 4, false, &[]);
    let var_info = block("VarFileInfo", &[], 0, true, &[translation]);
    block("VS_VERSION_INFO", &fixed, fixed.len() as u16, false, &[string_info, var_info])
}

/// The resource section: each image of the .ico as an RT_ICON (ids 1, 2, ...), one
/// RT_GROUP_ICON (id 1) listing them, and the RT_VERSION (id 1). Returns the section and where
/// its data addresses are.
fn rsrc(ico: &[u8]) -> (Vec<u8>, Vec<u32>) {
    let ver = version();
    let le32 = |i: usize| u32::from_le_bytes(ico[i..i + 4].try_into().unwrap());
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    // The group is the .ico's own header and entries, with each file offset swapped for an id.
    let mut group = ico[..6].to_vec();
    let mut items: Vec<(u32, u32, &[u8])> = vec![];
    for i in 0..count {
        let e = 6 + 16 * i;
        let (size, at) = (le32(e + 8) as usize, le32(e + 12) as usize);
        items.push((RT_ICON, i as u32 + 1, &ico[at..at + size]));
        group.extend_from_slice(&ico[e..e + 12]);
        put16(&mut group, i as u16 + 1);
    }
    items.push((RT_GROUP_ICON, 1, &group[..]));
    items.push((RT_VERSION, 1, &ver[..]));

    // Windows reads a tree: type -> id -> language -> data entry. Lay out every table first.
    let dir = |entries: usize| 16 + 8 * entries;
    let types = [RT_ICON, RT_GROUP_ICON, RT_VERSION]; // ascending, as Windows expects
    let of_type = |t: u32| items.iter().filter(move |it| it.0 == t).count();
    let mut at = dir(types.len());
    let mut take = |n: usize, align: usize| {
        at = at.next_multiple_of(align) + n;
        at - n
    };
    let type_dirs: Vec<usize> = types.iter().map(|&t| take(dir(of_type(t)), 1)).collect();
    let lang_dirs: Vec<usize> = items.iter().map(|_| take(dir(1), 1)).collect();
    let entries: Vec<usize> = items.iter().map(|_| take(16, 1)).collect();
    let datas: Vec<usize> = items.iter().map(|it| take(it.2.len(), 8)).collect();

    let mut s = vec![];
    let head = |s: &mut Vec<u8>, n: usize| {
        s.extend_from_slice(&[0; 12]); // characteristics, time stamp, version
        put16(s, 0); // named entries
        put16(s, n as u16); // numbered entries
    };
    let link = |s: &mut Vec<u8>, id: u32, to: usize, table: bool| {
        put32(s, id);
        put32(s, to as u32 | if table { 0x8000_0000 } else { 0 });
    };
    head(&mut s, types.len());
    for (&t, &d) in types.iter().zip(&type_dirs) {
        link(&mut s, t, d, true);
    }
    for &t in &types {
        head(&mut s, of_type(t));
        for (i, it) in items.iter().enumerate().filter(|(_, it)| it.0 == t) {
            link(&mut s, it.1, lang_dirs[i], true);
        }
    }
    for &e in &entries {
        head(&mut s, 1);
        link(&mut s, LANG_EN_US, e, false);
    }
    let mut fixups = vec![];
    for (it, &d) in items.iter().zip(&datas) {
        fixups.push(s.len() as u32); // an address in the image: the linker fills it in
        put32(&mut s, d as u32);
        put32(&mut s, it.2.len() as u32);
        put32(&mut s, 0); // code page
        put32(&mut s, 0);
    }
    for (it, &d) in items.iter().zip(&datas) {
        s.resize(d, 0);
        s.extend_from_slice(it.2);
    }
    (s, fixups)
}

/// A one-section COFF object (x64). Each fixup is an image-relative address of a spot in the
/// section itself, so it points at the section's own symbol.
fn coff((data, fixups): &(Vec<u8>, Vec<u32>)) -> Vec<u8> {
    let relocs = 20 + 40 + data.len() as u32;
    let symbols = relocs + 10 * fixups.len() as u32;
    let mut o = vec![];
    put16(&mut o, 0x8664); // machine: x64
    put16(&mut o, 1); // sections
    put32(&mut o, 0); // time stamp
    put32(&mut o, symbols);
    put32(&mut o, 2); // the section symbol and its auxiliary record
    put16(&mut o, 0); // optional header size
    put16(&mut o, 0); // characteristics

    o.extend_from_slice(b".rsrc\0\0\0");
    put32(&mut o, 0); // virtual size
    put32(&mut o, 0); // virtual address
    put32(&mut o, data.len() as u32);
    put32(&mut o, 20 + 40); // raw data right after this header
    put32(&mut o, relocs);
    put32(&mut o, 0); // line numbers
    put16(&mut o, fixups.len() as u16);
    put16(&mut o, 0);
    put32(&mut o, 0x4000_0040); // initialized data, readable

    o.extend_from_slice(data);
    for &f in fixups {
        put32(&mut o, f);
        put32(&mut o, 0); // symbol 0: the section
        put16(&mut o, 3); // IMAGE_REL_AMD64_ADDR32NB
    }

    o.extend_from_slice(b".rsrc\0\0\0");
    put32(&mut o, 0); // value
    put16(&mut o, 1); // section number
    put16(&mut o, 0); // type
    o.push(3); // storage class: static
    o.push(1); // one auxiliary record:
    put32(&mut o, data.len() as u32);
    put16(&mut o, fixups.len() as u16);
    o.extend_from_slice(&[0; 12]);
    put32(&mut o, 4); // empty string table
    o
}
