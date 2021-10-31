use structopt::StructOpt;

use dwarfldr::{Type, Variant, VariantShape, Encoding};

#[derive(Debug, StructOpt)]
struct TySh {
    filename: std::path::PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = TySh::from_args();

    let buffer = std::fs::read(args.filename)?;
    let object = object::File::parse(&*buffer)?;
    let everything = dwarfldr::parse_file(&object)?;

    println!("Loaded; {} types found in program.", everything.type_count());
    println!("To quit: ^D or exit");

    let mut rl = rustyline::Editor::<()>::new();
    let prompt = ansi_term::Colour::Green.paint(">> ").to_string();
    'lineloop:
    loop {
        match rl.readline(&prompt) {
            Ok(line) => {
                let line = line.trim();
                let (cmd, rest) = line.split_once(char::is_whitespace)
                    .unwrap_or((line, ""));
                if line.is_empty() {
                    continue 'lineloop;
                }

                rl.add_history_entry(line);

                match cmd {
                    "exit" => break,
                    "help" => {
                        println!("commands:");
                        for (name, _, desc) in COMMANDS {
                            println!("{:12} {}", name, desc);
                        }
                    }
                    _ => {
                        for (name, imp, _) in COMMANDS {
                            if *name == cmd {
                                imp(&everything, rest);
                                continue 'lineloop;
                            }
                        }
                        println!("unknown command: {}", cmd);
                        println!("for help, try: help");
                    }
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(e) => {
                println!("{:?}", e);
                break;
            }
        }
    }

    Ok(())
}

struct Goff(gimli::UnitSectionOffset);

impl std::fmt::Display for Goff {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.0 {
            gimli::UnitSectionOffset::DebugInfoOffset(gimli::DebugInfoOffset(x)) => {
                write!(f, "<.debug_info+0x{:08x}>", x)
            }
            gimli::UnitSectionOffset::DebugTypesOffset(gimli::DebugTypesOffset(x)) => {
                write!(f, "<.debug_types+0x{:08x}>", x)
            }
        }
    }
}

struct NamedGoff<'a>(&'a dwarfldr::Types, gimli::UnitSectionOffset);

impl std::fmt::Display for NamedGoff<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let bold = ansi_term::Style::new().bold();
        let dim = ansi_term::Style::new().dimmed();

        let n = if let Some(name) = self.0.name_from_goff(self.1) {
            name
        } else {
            "<anonymous type>".into()
        };

        write!(f, "{}", bold.paint(n))?;
        match self.1 {
            gimli::UnitSectionOffset::DebugInfoOffset(gimli::DebugInfoOffset(x)) => {
                write!(f, " {}<.debug_info+0x{:08x}>{}", dim.prefix(), x, dim.suffix())
            }
            gimli::UnitSectionOffset::DebugTypesOffset(gimli::DebugTypesOffset(x)) => {
                write!(f, " {}<.debug_types+0x{:08x}>{}", dim.prefix(), x, dim.suffix())
            }
        }
    }
}

type Command = fn(&dwarfldr::Types, &str);

static COMMANDS: &[(&str, Command, &str)] = &[
    ("list", cmd_list, "print names of ALL types, or types containing a string"),
    ("info", cmd_info, "print a summary of a type"),
    ("def", cmd_def, "print a type as a pseudo-Rust definition"),
    ("sizeof", cmd_sizeof, "print size of type in bytes"),
    ("alignof", cmd_alignof, "print alignment of type in bytes"),
    ("addr2line", cmd_addr2line, "look up line number information"),
];

fn cmd_list(
    db: &dwarfldr::Types,
    args: &str,
) {
    for (goff, _) in db.types() {
        if !args.is_empty() {
            if let Some(name) = db.name_from_goff(goff) {
                if !name.contains(args) {
                    continue;
                }
            }
        }

        println!("{}", NamedGoff(db, goff));
    }
}

fn parse_type_name(s: &str) -> Option<ParsedTypeName<'_>> {
    if s.starts_with("<.debug_") && s.ends_with('>') {
        // Try parsing as a debug section reference.
        let rest = &s[8..];
        return if rest.starts_with("info+0x") {
            let num = &rest[7..rest.len() - 1];
            if let Ok(n) = usize::from_str_radix(num, 16) {
                Some(ParsedTypeName::Goff(gimli::DebugInfoOffset(n).into()))
            } else {
                println!("can't parse {} as hex", num);
                None
            }
        } else if rest.starts_with("types+0x") {
            let num = &rest[8..rest.len() - 1];
            if let Ok(n) = usize::from_str_radix(num, 16) {
                Some(ParsedTypeName::Goff(gimli::DebugTypesOffset(n).into()))
            } else {
                println!("can't parse {} as hex", num);
                None
            }
        } else {
            println!("bad offset reference: {}", s);
            None
        };
    }

    Some(ParsedTypeName::Name(s))
}

enum ParsedTypeName<'a> {
    Name(&'a str),
    Goff(gimli::UnitSectionOffset),
}

fn simple_query_cmd(
    db: &dwarfldr::Types,
    args: &str,
    q: fn(&dwarfldr::Types, &dwarfldr::Type),
) {
    let type_name = args.trim();
    let types: Vec<_> = match parse_type_name(type_name) {
        None => return,
        Some(ParsedTypeName::Name(n)) => {
            db.types_by_name(n).collect()
        }
        Some(ParsedTypeName::Goff(o)) => {
            db.type_from_goff(o).into_iter()
                .map(|t| (o, t))
                .collect()
        }
    };
    if type_name.starts_with("<.debug_") && type_name.ends_with('>') {
        // Try parsing as a debug section reference.
        let rest = &type_name[8..];
        if rest.starts_with("info+0x") {
        } else if rest.starts_with("types+0x") {
        }
    }

    let many = match types.len() {
        0 => {
            println!("{}", ansi_term::Colour::Red.paint("No types found."));
            return;
        }
        1 => false,
        n => {
            println!("{}{} types found with that name:",
                ansi_term::Color::Yellow.paint("note: "),
                n,
            );
            true
        }
    };

    for (goff, t) in types {
        if many { println!() }
        print!("{}: ", NamedGoff(db, goff));
        q(db, t);
    }
}

fn cmd_info(db: &dwarfldr::Types, args: &str) {
    simple_query_cmd(db, args, |db, t| {
        match t {
            Type::Base(s) => {
                println!("base type");
                println!("- encoding: {:?}", s.encoding);
                println!("- byte size: {}", s.byte_size);
            }
            Type::Pointer(s) => {
                println!("pointer type");
                println!("- points to: {}", NamedGoff(db, s.ty_goff));
            }
            Type::Array(s) => {
                println!("array type");
                println!("- element type: {}", NamedGoff(db, s.element_ty_goff));
                println!("- lower bound: {}", s.lower_bound);
                if let Some(n) = s.count {
                    println!("- count: {}", n);
                } else {
                    println!("- size not given");
                }
            }
            Type::Struct(s) => {
                if s.tuple_like {
                    println!("struct type (tuple-like)");
                } else {
                    println!("struct type");
                }
                println!("- byte size: {}", s.byte_size);
                if let Some(a) = s.alignment {
                    println!("- alignment: {}", a);
                } else {
                    println!("- not aligned");
                }
                if !s.template_type_parameters.is_empty() {
                    println!("- template type parameters:");
                    for ttp in &s.template_type_parameters {
                        println!("  - {} = {}", ttp.name, NamedGoff(db, ttp.ty_goff));
                    }
                }
                if !s.members.is_empty() {
                    println!("- members:");
                    for mem in s.members.values() {
                        if let Some(name) = &mem.name {
                            println!("  - {}: {}", name, NamedGoff(db, mem.ty_goff));
                        } else {
                            println!("  - <unnamed>: {}", NamedGoff(db, mem.ty_goff));
                        }
                        println!("    - offset: {} bytes", mem.location);
                        if let Some(a) = mem.alignment {
                            println!("    - aligned: {} bytes", a);
                        }
                        if mem.artificial {
                            println!("    - artificial");
                        }
                    }
                } else {
                    println!("- no members");
                }
            }
            Type::Enum(s) => {
                println!("enum type");
                println!("- byte size: {}", s.byte_size);
                if let Some(a) = s.alignment {
                    println!("- alignment: {}", a);
                } else {
                    println!("- not aligned");
                }
                if !s.template_type_parameters.is_empty() {
                    println!("- type parameters:");
                    for ttp in &s.template_type_parameters {
                        println!("  - {} = {}", ttp.name, NamedGoff(db, ttp.ty_goff));
                    }
                }

                match &s.variant_part.shape {
                    dwarfldr::VariantShape::Zero => {
                        println!("- empty (uninhabited) enum");
                    }
                    dwarfldr::VariantShape::One(v) => {
                        println!("- single variant enum w/o discriminator");
                        println!("  - content type: {}", NamedGoff(db, v.member.ty_goff));
                        println!("  - offset: {} bytes", v.member.location);
                        if let Some(a) = v.member.alignment {
                            println!("  - aligned: {} bytes", a);
                        }
                        if !v.member.artificial {
                            println!("  - not artificial, oddly");
                        }
                    }
                    dwarfldr::VariantShape::Many { member, variants, .. }=> {
                        if let Some(dname) = db.name_from_goff(member.ty_goff) {
                            println!("- {} variants discriminated by {} at offset {}", variants.len(), dname, member.location);
                        } else {
                            println!("- {} variants discriminated by an anonymous type at offset {}", variants.len(), member.location);
                        }
                        if !member.artificial {
                            println!("  - not artificial, oddly");
                        }
                        
                        // Print explicit values first
                        for (val, var) in variants {
                            if let Some(val) = val {
                                println!("- when discriminator == {}", val);
                                println!("  - contains type: {}", NamedGoff(db, var.member.ty_goff));
                                println!("  - at offset: {} bytes", var.member.location);
                                if let Some(a) = var.member.alignment {
                                    println!("  - aligned: {} bytes", a);
                                }
                            }
                        }
                        // Now, default.
                        for (val, var) in variants {
                            if val.is_none() {
                                println!("- any other discriminator value");
                                println!("  - contains type: {}", NamedGoff(db, var.member.ty_goff));
                                println!("  - at offset: {} bytes", var.member.location);
                                if let Some(a) = var.member.alignment {
                                    println!("  - aligned: {} bytes", a);
                                }
                            }
                        }
                    }
                }
            }
            Type::CEnum(s) => {
                println!("C-like enum type");
                println!("- byte size: {}", s.byte_size);
                println!("- alignment: {}", s.alignment);
                println!("- {} values defined", s.enumerators.len());
                for e in s.enumerators.values() {
                    println!("  - {} = 0x{:x}", e.name, e.const_value);

                }
            }
            Type::Union(s) => {
            }
            Type::Subroutine(s) => {
            }
        }
    })
}

fn cmd_sizeof(db: &dwarfldr::Types, args: &str) {
    simple_query_cmd(db, args, |db, t| {
        if let Some(sz) = t.byte_size(db) {
            println!("{} bytes", sz);
        } else {
            println!("unsized");
        }
    })
}

fn cmd_alignof(db: &dwarfldr::Types, args: &str) {
    simple_query_cmd(db, args, |db, t| {
        if let Some(sz) = t.alignment(db) {
            println!("align to {} bytes", sz);
        } else {
            println!("no alignment information");
        }
    })
}

fn cmd_def(db: &dwarfldr::Types, args: &str) {
    simple_query_cmd(db, args, |db, t| {
        println!();
        match t {
            Type::Base(s) => {
                print!("type _ = ");
                match (s.encoding, s.byte_size) {
                    (_, 0) => print!("()"),
                    (Encoding::Unsigned, 1) => print!("u8"),
                    (Encoding::Unsigned, 2) => print!("u16"),
                    (Encoding::Unsigned, 4) => print!("u32"),
                    (Encoding::Unsigned, 8) => print!("u64"),
                    (Encoding::Unsigned, 16) => print!("u128"),
                    (Encoding::Signed, 1) => print!("i8"),
                    (Encoding::Signed, 2) => print!("i16"),
                    (Encoding::Signed, 4) => print!("i32"),
                    (Encoding::Signed, 8) => print!("i64"),
                    (Encoding::Signed, 16) => print!("i128"),
                    (Encoding::Float, 4) => print!("f32"),
                    (Encoding::Float, 8) => print!("f64"),
                    (Encoding::Boolean, 1) => print!("bool"),
                    (Encoding::UnsignedChar, 4) => print!("char"),
                    (Encoding::UnsignedChar, 1) => print!("c_uchar"),
                    (Encoding::SignedChar, 1) => print!("c_schar"),

                    (e, s) => print!("Unhandled{:?}{}", e, s),
                }
                println!(";");
            }
            Type::Pointer(s) => {
                println!("{}", s.name);
            }
            Type::Array(s) => {
                let name = db.name_from_goff(s.element_ty_goff).unwrap();
                if let Some(n) = s.count {
                    println!("[{}; {}]", name, n);
                } else {
                    println!("[{}]", name);
                }
            }
            Type::Struct(s) => {
                print!("struct {}", s.name);

                if !s.template_type_parameters.is_empty() {
                    print!("<");
                    for ttp in &s.template_type_parameters {
                        print!("{},", ttp.name);
                    }
                    print!(">");
                }
                
                if s.members.is_empty() {
                    println!(";");
                } else {
                    if s.tuple_like {
                        println!("(");
                        for mem in s.members.values() {
                            println!("    {},", db.name_from_goff(mem.ty_goff).unwrap());
                        }
                        println!(");");
                    } else {
                        println!(" {{");
                        for mem in s.members.values() {
                            if let Some(name) = &mem.name {
                                println!("    {}: {},", name, db.name_from_goff(mem.ty_goff).unwrap());
                            } else {
                                println!("    ANON: {},", db.name_from_goff(mem.ty_goff).unwrap());
                            }
                        }
                        println!("}}");
                    }
                }
            }
            Type::Enum(s) => {
                print!("enum {}", s.name);
                if !s.template_type_parameters.is_empty() {
                    print!("<");
                    for ttp in &s.template_type_parameters {
                        print!("{}", ttp.name);
                    }
                    print!(">");
                }
                println!(" {{");

                match &s.variant_part.shape {
                    dwarfldr::VariantShape::Zero => (),
                    dwarfldr::VariantShape::One(var) => {
                        if let Some(name) = &var.member.name {
                            print!("    {}", name);
                        } else {
                            print!("    ANON");
                        }

                        let mty = db.type_from_goff(var.member.ty_goff)
                            .unwrap();
                        if let Type::Struct(s) = mty {
                            if !s.members.is_empty() {
                                if s.tuple_like {
                                    println!("(");
                                    for mem in s.members.values() {
                                        let mtn = db.name_from_goff(mem.ty_goff).unwrap();
                                        println!("        {},", mtn);
                                    }
                                    print!("    )");
                                } else {
                                    println!(" {{");
                                    for mem in s.members.values() {
                                        let mtn = db.name_from_goff(mem.ty_goff).unwrap();
                                        println!("        {}: {},", mem.name.as_ref().unwrap(), mtn);
                                    }
                                    print!("    }}");
                                }
                            }
                        } else {
                            print!("(unexpected weirdness)");
                        }

                        println!(",");
                    }
                    dwarfldr::VariantShape::Many { variants, .. }=> {
                        for var in variants.values() {
                            if let Some(name) = &var.member.name {
                                print!("    {}", name);
                            } else {
                                print!("    ANON");
                            }

                            let mty = db.type_from_goff(var.member.ty_goff)
                                .unwrap();
                            if let Type::Struct(s) = mty {
                                if !s.members.is_empty() {
                                    if s.tuple_like {
                                        println!("(");
                                        for mem in s.members.values() {
                                            let mtn = db.name_from_goff(mem.ty_goff).unwrap();
                                            println!("        {},", mtn);
                                        }
                                        print!("    )");
                                    } else {
                                        println!(" {{");
                                        for mem in s.members.values() {
                                            let mtn = db.name_from_goff(mem.ty_goff).unwrap();
                                            println!("        {}: {},", mem.name.as_ref().unwrap(), mtn);
                                        }
                                        print!("    }}");
                                    }
                                }
                            } else {
                                print!("(unexpected weirdness)");
                            }

                            println!(",");
                        }
                    }
                }
                println!("}}");

            }
            Type::CEnum(s) => {
                println!("enum {} {{", s.name);
                for (val, e) in &s.enumerators {
                    println!("    {} = 0x{:x},", e.name, val);
                }
                println!("}}");
            }
            Type::Union(s) => {
            }
            Type::Subroutine(s) => {
                println!("fn(");
                for &p in &s.formal_parameters {
                    println!("    {},", db.name_from_goff(p).unwrap());
                }
                if let Some(rt) = s.return_ty_goff {
                    println!(") -> {} {{", db.name_from_goff(rt).unwrap());
                } else {
                    println!(") {{");
                }
                println!("    // code goes here");
                println!("    // (this is a subroutine type, _not_ a fn ptr)");
                println!("    unimplemented!();");
                println!("}}");
            }
        }
    })
}

fn cmd_addr2line(db: &dwarfldr::Types, args: &str) {
    let addr = if args.starts_with("0x") {
        if let Ok(a) = u64::from_str_radix(&args[2..], 16) {
            a
        } else {
            println!("can't parse {} as an address", args);
            return;
        }
    } else if let Ok(a) = args.parse::<u64>() {
        a
    } else {
        println!("can't parse {} as an address", args);
        return;
    };

    if let Some(row) = db.lookup_line_row(addr) {
        print!("{}:", row.file);
        if let Some(line) = row.line {
            print!("{}:", line);
        } else {
            print!("?:");
        }
        if let Some(col) = row.column {
            print!("{}", col);
        } else {
            print!("?");
        }
        println!();
    } else {
        println!("no line number information available for address");
    }
}