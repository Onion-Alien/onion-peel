//! Puts res/onion-peel.ico into onion-peel.exe as its icon, with no resource compiler: the icon is
//! written as a COFF object holding a `.rsrc` section, which the linker takes as it is.

use std::path::Path;

const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
const LANG_EN_US: u32 = 0x409;

fn main() {
    println!("cargo:rerun-if-changed=res/onion-peel.ico");
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

/// The resource section: each image of the .ico as an RT_ICON (ids 1, 2, ...) and one
/// RT_GROUP_ICON (id 1) listing them. Returns the section and where its data addresses are.
fn rsrc(ico: &[u8]) -> (Vec<u8>, Vec<u32>) {
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

    // Windows reads a tree: type -> id -> language -> data entry. Lay out every table first.
    let dir = |entries: usize| 16 + 8 * entries;
    let types = [RT_ICON, RT_GROUP_ICON];
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
