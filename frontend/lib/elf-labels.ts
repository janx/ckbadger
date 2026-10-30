/** ELF program-header `p_type` names used on the code-cell board (SC-K1). */
const PROGRAM_TYPES: Record<number, string> = {
  0: 'NULL',
  1: 'LOAD',
  2: 'DYNAMIC',
  3: 'INTERP',
  4: 'NOTE',
  5: 'SHLIB',
  6: 'PHDR',
  7: 'TLS',
  0x6474e550: 'GNU_EH_FRAME',
  0x6474e551: 'GNU_STACK',
  0x6474e552: 'GNU_RELRO',
  0x70000003: 'RISCV_ATTRIBUTES',
};

/** ELF section-header `sh_type` names. */
const SECTION_TYPES: Record<number, string> = {
  0: 'NULL',
  1: 'PROGBITS',
  2: 'SYMTAB',
  3: 'STRTAB',
  4: 'RELA',
  5: 'HASH',
  6: 'DYNAMIC',
  7: 'NOTE',
  8: 'NOBITS',
  9: 'REL',
  10: 'SHLIB',
  11: 'DYNSYM',
  14: 'INIT_ARRAY',
  15: 'FINI_ARRAY',
  16: 'PREINIT_ARRAY',
  17: 'GROUP',
  18: 'SYMTAB_SHNDX',
  0x70000003: 'RISCV_ATTRIBUTES',
};

/** ELF header `e_type` names (group scripts, SC-K4). */
const OBJECT_TYPES: Record<number, string> = {
  0: 'NONE',
  1: 'REL',
  2: 'EXEC',
  3: 'DYN',
  4: 'CORE',
};

const MACHINES: Record<number, string> = {
  243: 'RISC-V',
};

function namedOrHex(map: Record<number, string>, value: number): string {
  return map[value] ?? `0x${value.toString(16)}`;
}

export function elfMachineLabel(machine: number): string {
  return namedOrHex(MACHINES, machine);
}

export function elfProgramTypeLabel(kind: number): string {
  return namedOrHex(PROGRAM_TYPES, kind);
}

export function elfSectionTypeLabel(kind: number): string {
  return namedOrHex(SECTION_TYPES, kind);
}

export function elfTypeLabel(kind: number): string {
  return `${kind} = ${namedOrHex(OBJECT_TYPES, kind)}`;
}
