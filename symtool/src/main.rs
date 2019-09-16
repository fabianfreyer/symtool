use clap::{
    app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg, ArgMatches,
};
use goblin::elf::sym::{Sym, STV_DEFAULT, STV_HIDDEN};
use goblin::mach::symbols::{Nlist, N_PEXT, N_STAB};
use hashbrown::HashMap;
use regex::RegexSet;
use std::io::Write;
use std::ops::Deref;

use symtool_backend as backend;

mod error;
use crate::error::Error;

fn main() {
    let matches = app_from_crate!()
        .arg(
            Arg::with_name("verbose")
                .long("verbose")
                .short("v")
                .help("Print information for each operation performed"),
        )
        .arg(
            Arg::with_name("rename")
                .long("rename")
                .number_of_values(2)
                .multiple(true)
                .value_names(&["OLD-NAME", "NEW-NAME"])
                .help("Renames symbols named OLD-NAME to NEW-NAME")
                .long_help("Renames symbols named OLD-NAME to NEW-NAME. Since string tables are simply patched and not rewritten, NEW-NAME must not have more characters than OLD-NAME")
        )
        .arg(
            Arg::with_name("hidden")
                .long("hidden")
                .takes_value(true)
                .value_name("PATTERN")
                .help("Sets all symbols with names matching regex PATTERN to hidden visibility"),
        )
        .arg(
            Arg::with_name("default")
                .long("default")
                .takes_value(true)
                .value_name("PATTERN")
                .help("Sets all symbols with names matching regex PATTERN to default visibility")
                .long_help(
                    "Sets all symbols with names matching regex PATTERN to default visibility.  --default takes precedance over --hidden when both patterns match a symbol name.",
                ),
        )
        .arg(
            Arg::with_name("INPUT")
                .help("Path to source object or archive file")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("OUTPUT")
                .help("Path to output file")
                .required(true)
                .index(2),
        )
        .get_matches();

    run(&matches).unwrap_or_else(|e| {
        writeln!(std::io::stderr(), "error: {}", e).unwrap();
        std::process::exit(-1)
    });
}

fn make_sym_hidden(s: &Sym, name: &str, verbose: bool) -> Sym {
    if verbose {
        println!("Set visibility hidden: {}", name);
    }
    Sym {
        st_other: (s.st_other & 0xfc) | STV_HIDDEN,
        ..s.clone()
    }
}

fn make_sym_default(s: &Sym, name: &str, verbose: bool) -> Sym {
    if verbose {
        println!("Set visibility default: {}", name);
    }
    Sym {
        st_other: (s.st_other & 0xfc) | STV_DEFAULT,
        ..s.clone()
    }
}

fn make_nlist_hidden(s: &Nlist, name: &str, verbose: bool) -> Option<Nlist> {
    if s.n_type & N_STAB != 0u8 {
        None
    } else {
        if verbose {
            println!("Set visibility hidden: {}", name);
        }
        Some(Nlist {
            n_type: s.n_type | N_PEXT,
            ..s.clone()
        })
    }
}

fn make_nlist_default(s: &Nlist, name: &str, verbose: bool) -> Option<Nlist> {
    if s.n_type & N_STAB != 0u8 {
        None
    } else {
        if verbose {
            println!("Set visibility default: {}", name);
        }
        Some(Nlist {
            n_type: s.n_type & !N_PEXT,
            ..s.clone()
        })
    }
}

pub fn run(matches: &ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let verbose = matches.is_present("verbose");
    let hidden_regex = matches
        .values_of("hidden")
        .map(|regexes| RegexSet::new(regexes))
        .transpose()?;
    let default_regex = matches
        .values_of("default")
        .map(|regexes| RegexSet::new(regexes))
        .transpose()?;
    let mut rename_map = HashMap::new();
    if let Some(rename) = matches.values_of("rename") {
        let original = rename.clone().step_by(2);
        let renamed = rename.skip(1).step_by(2);
        for (old, new) in original.zip(renamed) {
            if new.len() > old.len() {
                return Err(Box::new(Error::Message(format!("Replacement symbol names cannot have more characters than the original name. Symbol '{}' cannot be renamed to '{}'.", old, new))));
            }
            rename_map.insert(old.to_string(), new.to_string());
        }
    }

    let transform: Box<backend::object::ObjectTransform<crate::error::Error>> =
        Box::new(move |bytes, object| {
            let mut patches = Vec::new();
            match object {
                backend::object::Object::Elf(elf) => {
                    if let Some(iter) = backend::elf::SymtabIter::symtab_from_elf(bytes, &elf)? {
                        for (ref name, ref sym) in
                            iter.collect::<backend::error::Result<Vec<_>>>()?
                        {
                            let debug_name = name.as_ref().map_or("unnamed symbol", |x| &x);
                            let (new_name, new_sym) = if let Some(name) = name {
                                let new_name = rename_map.get(*name.deref());
                                let new_sym = if default_regex.is_some()
                                    && default_regex.as_ref().unwrap().is_match(name)
                                {
                                    Some(make_sym_default(sym, debug_name, verbose))
                                } else if hidden_regex.is_some()
                                    && hidden_regex.as_ref().unwrap().is_match(name)
                                {
                                    Some(make_sym_hidden(sym, debug_name, verbose))
                                } else {
                                    None
                                };
                                (new_name, new_sym)
                            } else {
                                (None, None)
                            };
                            if name.is_some() && new_name.is_some() {
                                patches.push(
                                    name.as_ref()
                                        .unwrap()
                                        .patch_with_bytes(new_name.unwrap().as_bytes())?,
                                );
                            }
                            if new_sym.is_some() {
                                patches.push(sym.patch_with(new_sym.unwrap())?);
                            }
                        }
                    }
                }
                backend::object::Object::MachO(mach) => {
                    if let Some(iter) = backend::mach::SymtabIter::from_mach(bytes, &mach) {
                        for (ref name, ref nlist) in
                            iter.collect::<backend::error::Result<Vec<_>>>()?
                        {
                            let debug_name = name.as_ref().map_or("unnamed symbol", |x| &x);
                            let (new_name, new_nlist) = if let Some(name) = name {
                                let new_name = rename_map.get(*name.deref());
                                let new_nlist = if default_regex.is_some()
                                    && default_regex.as_ref().unwrap().is_match(name)
                                {
                                    make_nlist_default(nlist, debug_name, verbose)
                                } else if hidden_regex.is_some()
                                    && hidden_regex.as_ref().unwrap().is_match(name)
                                {
                                    make_nlist_hidden(nlist, debug_name, verbose)
                                } else {
                                    None
                                };
                                (new_name, new_nlist)
                            } else {
                                (None, None)
                            };
                            if name.is_some() && new_name.is_some() {
                                patches.push(
                                    name.as_ref()
                                        .unwrap()
                                        .patch_with_bytes(new_name.unwrap().as_bytes())?,
                                );
                            }
                            if new_nlist.is_some() {
                                patches.push(nlist.patch_with(new_nlist.unwrap())?);
                            }
                        }
                    }
                }
            }
            Ok(patches)
        });

    let mut input = std::fs::File::open(matches.value_of("INPUT").unwrap())?;
    let mut output = std::fs::File::create(matches.value_of("OUTPUT").unwrap())?;
    backend::object::transform_object(&mut input, &mut output, &transform)?;
    Ok(())
}