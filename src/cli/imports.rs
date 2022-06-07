use colored::Colorize;
use std::fmt;

#[derive(Default, Debug, PartialEq, Eq, Clone)]
struct Import {
    name: String,
    source: String,
    address: u64,
    attrs: Vec<String>,
    no_color: bool,
}

impl Import {
    fn from_elf(elf: &goblin::elf::Elf, reloc: &goblin::elf::Reloc, no_color: bool) -> Import {
        let sym_idx = reloc.r_sym;
        let sym = elf.dynsyms.get(sym_idx).unwrap(); //panic is unlikely, but could be handled better

        let mut imp = Import::default();

        imp.name = match elf.dynstrtab.get_at(sym.st_name) {
            Some(s) => s.to_string(),
            None => "".to_string(),
        };

        imp.source = get_elf_symbol_version_string(&elf, sym_idx)
            .unwrap_or_else(|| "Unable to parse source".to_string());

        imp.address = reloc.r_offset;

        let symbol_r_type = match elf.header.e_machine {
            goblin::elf::header::EM_X86_64 => get_symbol_r_type_64(reloc.r_type),
            goblin::elf::header::EM_386 => get_symbol_r_type_32(reloc.r_type),
            _ => reloc.r_type.to_string(),
        };

        imp.attrs = vec![
            symbol_r_type,
            match get_plt_address(elf, &reloc) {
                Some(a) => format!("{:#x}", a),
                None => "".to_string(),
            },
            sym_idx.to_string(),
            format!("{:#x}", sym.st_value),
        ];

        if let Some(addend) = reloc.r_addend {
            imp.attrs.push(addend.to_string())
        }

        imp.no_color = no_color;

        imp
    }

    fn from_pe(import: &goblin::pe::import::Import, no_color: bool) -> Import {
        let mut imp = Import::default();

        imp.name = import.name.to_string();
        imp.source = import.dll.to_string();
        imp.address = import.rva as u64;

        let offset = format!("{:#x}", import.offset);

        imp.attrs = vec![import.ordinal.to_string(), offset];

        imp.no_color = no_color;

        imp
    }

    fn from_macho(import: goblin::mach::imports::Import, no_color: bool) -> Import {
        let mut imp = Import::default();

        imp.name = import.name.to_string();
        imp.source = import.dylib.to_string();
        imp.address = import.address;

        let offset = format!("{:#x}", import.offset);
        let seq_offset = format!("{:#x}", import.start_of_sequence_offset);

        imp.attrs = vec![
            offset,
            seq_offset,
            import.addend.to_string(),
            import.is_lazy.to_string(),
            import.is_weak.to_string(),
        ];

        imp.no_color = no_color;

        imp
    }
}

impl fmt::Display for Import {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let color_punctuation = |s: &str| {
            if self.no_color {
                s.normal()
            } else {
                s.bright_magenta()
            }
        };

        let single_quote = color_punctuation("'");
        let colon = color_punctuation(":");

        write!(
            f,
            "{}{}{}{}  {}  {}  {}",
            single_quote,
            {
                match self.no_color {
                    true => format!("{}", self.name).normal(),
                    false => format!("{}", self.name).yellow(),
                }
            },
            single_quote,
            colon,
            {
                match self.no_color {
                    true => format!("{}", self.source).normal(),
                    false => format!("{}", self.source).green(),
                }
            },
            {
                match self.no_color {
                    true => format!("{:#x}", self.address).normal(),
                    false => format!("{:#x}", self.address).red(),
                }
            },
            format!("{:?}", self.attrs).normal(),
        )
    }
}

// Dump Functions

pub fn dump_elf_imports(elf: &goblin::elf::Elf, no_color: bool) {
    println!("Procedural Linkage Table (PLT) symbols:");
    print!("\tName: Source, Version  Address  [Reloc type, .plt address, Idx, Value");

    if elf.dynamic.as_ref().unwrap().info.pltrel == goblin::elf::dynamic::DT_RELA {
        println!(", Addend]");
    } else {
        println!("]");
    }

    for reloc in elf.pltrelocs.iter() {
        println!("\t{}", Import::from_elf(elf, &reloc, no_color))
    }

    println!("\nOther dynamic symbols:");
    print!("\tName: Source, Version  Address  [Reloc type, .plt address, Idx, Value");

    if elf.dynamic.as_ref().unwrap().info.pltrel == goblin::elf::dynamic::DT_RELA {
        println!(", Addend]");
        for reloc in elf.dynrelas.iter() {
            println!("\t{}", Import::from_elf(elf, &reloc, no_color))
        }
    } else {
        println!("]");
        for reloc in elf.dynrels.iter() {
            println!("\t{}", Import::from_elf(elf, &reloc, no_color))
        }
    }
}

pub fn dump_pe_imports(pe: &goblin::pe::PE, no_color: bool) {
    println!("Imports:");
    println!("\t'name': dll rva [ordinal, offset]");
    for import in pe.imports.iter().as_ref() {
        println!("\t{}", Import::from_pe(import, no_color));
    }
}

pub fn dump_macho_imports(macho: &goblin::mach::MachO, no_color: bool) {
    println!("Imports:");
    println!("\t'name': dylib address [offset, start of sequence offset, addend, is lazily evaluated?, is weak?]");
    for import in macho.imports().expect("Error parsing imports") {
        println!("\t{}", Import::from_macho(import, no_color));
    }
}

// ELF Helper Functions

fn get_elf_symbol_version_string(elf: &goblin::elf::Elf, sym_idx: usize) -> Option<String> {
    let versym = elf.versym.as_ref()?.get_at(sym_idx)?;

    if versym.is_local() {
        return Some("local".to_string());
    } else if versym.is_global() {
        return Some("global".to_string());
    } else if let Some(needed) = elf
        .verneed
        .as_ref()?
        .iter()
        .find(|v| v.iter().any(|f| f.vna_other == versym.version()))
    {
        if let Some(version) = needed.iter().find(|f| f.vna_other == versym.version()) {
            let need_str = elf.dynstrtab.get_at(needed.vn_file)?;
            let vers_str = elf.dynstrtab.get_at(version.vna_name)?;
            return Some(format!("{}, {}", need_str, vers_str));
        }
    }

    None
}

fn get_plt_address(elf: &goblin::elf::Elf, reloc: &goblin::elf::Reloc) -> Option<u64> {
    // only handle JUMP_SLOT relocations
    if reloc.r_type == goblin::elf::reloc::R_X86_64_JUMP_SLOT {
        let reloc_idx = elf.pltrelocs.iter().position(|r| r == *reloc)?;
        let plt_stub_len = 16;
        let offset = (reloc_idx as u64 + 1) * plt_stub_len;

        let plt_base = &elf
            .section_headers
            .iter()
            .find(|s| elf.shdr_strtab.get_at(s.sh_name).unwrap().eq(".plt"))?
            .sh_addr;

        return Some(plt_base + offset);
    }

    None
}

fn get_symbol_r_type_64(sym_type: u32) -> String {
    match sym_type {
        goblin::elf::reloc::R_X86_64_NONE => "R_X86_64_NONE".to_string(),
        goblin::elf::reloc::R_X86_64_64 => "R_X86_64_64".to_string(),
        goblin::elf::reloc::R_X86_64_PC32 => "R_X86_64_PC32".to_string(),
        goblin::elf::reloc::R_X86_64_GOT32 => "R_X86_64_GOT32".to_string(),
        goblin::elf::reloc::R_X86_64_PLT32 => "R_X86_64_PLT32".to_string(),
        goblin::elf::reloc::R_X86_64_COPY => "R_X86_64_COPY".to_string(),
        goblin::elf::reloc::R_X86_64_GLOB_DAT => "R_X86_64_GLOB_DAT".to_string(),
        goblin::elf::reloc::R_X86_64_JUMP_SLOT => "R_X86_64_JUMP_SLOT".to_string(),
        goblin::elf::reloc::R_X86_64_RELATIVE => "R_X86_64_RELATIVE".to_string(),
        goblin::elf::reloc::R_X86_64_GOTPCREL => "R_X86_64_GOTPCREL".to_string(),
        goblin::elf::reloc::R_X86_64_32 => "R_X86_64_32".to_string(),
        goblin::elf::reloc::R_X86_64_32S => "R_X86_64_32S".to_string(),
        goblin::elf::reloc::R_X86_64_16 => "R_X86_64_16".to_string(),
        goblin::elf::reloc::R_X86_64_PC16 => "R_X86_64_PC16".to_string(),
        goblin::elf::reloc::R_X86_64_8 => "R_X86_64_8".to_string(),
        goblin::elf::reloc::R_X86_64_PC8 => "R_X86_64_PC8".to_string(),
        goblin::elf::reloc::R_X86_64_DTPMOD64 => "R_X86_64_DTPMOD64".to_string(),
        goblin::elf::reloc::R_X86_64_DTPOFF64 => "R_X86_64_DTPOFF64".to_string(),
        goblin::elf::reloc::R_X86_64_TPOFF64 => "R_X86_64_TPOFF64".to_string(),
        goblin::elf::reloc::R_X86_64_TLSGD => "R_X86_64_TLSGD".to_string(),
        goblin::elf::reloc::R_X86_64_TLSLD => "R_X86_64_TLSLD".to_string(),
        goblin::elf::reloc::R_X86_64_DTPOFF32 => "R_X86_64_DTPOFF32".to_string(),
        goblin::elf::reloc::R_X86_64_GOTTPOFF => "R_X86_64_GOTTPOFF".to_string(),
        goblin::elf::reloc::R_X86_64_TPOFF32 => "R_X86_64_TPOFF32".to_string(),
        goblin::elf::reloc::R_X86_64_PC64 => "R_X86_64_PC64".to_string(),
        goblin::elf::reloc::R_X86_64_GOTOFF64 => "R_X86_64_GOTOFF64".to_string(),
        goblin::elf::reloc::R_X86_64_GOTPC32 => "R_X86_64_GOTPC32".to_string(),
        goblin::elf::reloc::R_X86_64_SIZE32 => "R_X86_64_SIZE32".to_string(),
        goblin::elf::reloc::R_X86_64_SIZE64 => "R_X86_64_SIZE64".to_string(),
        goblin::elf::reloc::R_X86_64_GOTPC32_TLSDESC => "R_X86_64_GOTPC32_TLSDESC".to_string(),
        goblin::elf::reloc::R_X86_64_TLSDESC_CALL => "R_X86_64_TLSDESC_CALL".to_string(),
        goblin::elf::reloc::R_X86_64_TLSDESC => "R_X86_64_TLSDESC".to_string(),
        goblin::elf::reloc::R_X86_64_IRELATIVE => "R_X86_64_IRELATIVE".to_string(),
        _ => sym_type.to_string(),
    }
}

fn get_symbol_r_type_32(sym_type: u32) -> String {
    match sym_type {
        goblin::elf::reloc::R_386_8 => "R_386_8".to_string(),
        goblin::elf::reloc::R_386_16 => "R_386_16".to_string(),
        goblin::elf::reloc::R_386_32 => "R_386_32".to_string(),
        goblin::elf::reloc::R_386_32PLT => "R_386_32PLT".to_string(),
        goblin::elf::reloc::R_386_COPY => "R_386_COPY".to_string(),
        goblin::elf::reloc::R_386_GLOB_DAT => "R_386_GLOB_DAT".to_string(),
        goblin::elf::reloc::R_386_GOT32 => "R_386_GOT32".to_string(),
        goblin::elf::reloc::R_386_GOT32X => "R_386_GOT32X".to_string(),
        goblin::elf::reloc::R_386_GOTOFF => "R_386_GOTOFF".to_string(),
        goblin::elf::reloc::R_386_GOTPC => "R_386_GOTPC".to_string(),
        goblin::elf::reloc::R_386_IRELATIVE => "R_386_IRELATIVE".to_string(),
        goblin::elf::reloc::R_386_JMP_SLOT => "R_386_JMP_SLOT".to_string(),
        goblin::elf::reloc::R_386_NONE => "R_386_NONE".to_string(),
        goblin::elf::reloc::R_386_NUM => "R_386_NUM".to_string(),
        goblin::elf::reloc::R_386_PC8 => "R_386_PC8".to_string(),
        goblin::elf::reloc::R_386_PC16 => "R_386_PC16".to_string(),
        goblin::elf::reloc::R_386_PC32 => "R_386_PC32".to_string(),
        goblin::elf::reloc::R_386_PLT32 => "R_386_PLT32".to_string(),
        goblin::elf::reloc::R_386_RELATIVE => "R_386_RELATIVE".to_string(),
        goblin::elf::reloc::R_386_SIZE32 => "R_386_SIZE32".to_string(),
        goblin::elf::reloc::R_386_TLS_DESC => "R_386_TLS_DESC".to_string(),
        goblin::elf::reloc::R_386_TLS_DESC_CALL => "R_386_TLS_DESC_CALL".to_string(),
        goblin::elf::reloc::R_386_TLS_DTPMOD32 => "R_386_TLS_DTPMOD32".to_string(),
        goblin::elf::reloc::R_386_TLS_DTPOFF32 => "R_386_TLS_DTPOFF32".to_string(),
        goblin::elf::reloc::R_386_TLS_GD => "R_386_TLS_GD".to_string(),
        goblin::elf::reloc::R_386_TLS_GD_32 => "R_386_TLS_GD_32".to_string(),
        goblin::elf::reloc::R_386_TLS_GD_CALL => "R_386_TLS_GD_CALL".to_string(),
        goblin::elf::reloc::R_386_TLS_GD_POP => "R_386_TLS_GD_POP".to_string(),
        goblin::elf::reloc::R_386_TLS_GD_PUSH => "R_386_TLS_GD_PUSH".to_string(),
        goblin::elf::reloc::R_386_TLS_GOTDESC => "R_386_TLS_GOTDESC".to_string(),
        goblin::elf::reloc::R_386_TLS_GOTIE => "R_386_TLS_GOTIE".to_string(),
        goblin::elf::reloc::R_386_TLS_IE => "R_386_TLS_IE".to_string(),
        goblin::elf::reloc::R_386_TLS_IE_32 => "R_386_TLS_IE_32".to_string(),
        goblin::elf::reloc::R_386_TLS_LDM => "R_386_TLS_LDM".to_string(),
        goblin::elf::reloc::R_386_TLS_LDM_32 => "R_386_TLS_LDM_32".to_string(),
        goblin::elf::reloc::R_386_TLS_LDM_CALL => "R_386_TLS_LDM_CALL".to_string(),
        goblin::elf::reloc::R_386_TLS_LDM_POP => "R_386_TLS_LDM_POP".to_string(),
        goblin::elf::reloc::R_386_TLS_LDM_PUSH => "R_386_TLS_LDM_PUSH".to_string(),
        goblin::elf::reloc::R_386_TLS_LDO_32 => "R_386_TLS_LDO_32".to_string(),
        goblin::elf::reloc::R_386_TLS_LE => "R_386_TLS_LE".to_string(),
        goblin::elf::reloc::R_386_TLS_LE_32 => "R_386_TLS_LE_32".to_string(),
        goblin::elf::reloc::R_386_TLS_TPOFF => "R_386_TLS_TPOFF".to_string(),
        goblin::elf::reloc::R_386_TLS_TPOFF32 => "R_386_TLS_TPOFF32".to_string(),
        _ => sym_type.to_string(),
    }
}
