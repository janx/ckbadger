import { describe, expect, it } from 'vitest';
import { elfMachineLabel, elfProgramTypeLabel } from '@/lib/elf-labels';

describe('ELF enum labels', () => {
  it('maps RISC-V machine and common header kinds to names', () => {
    expect(elfMachineLabel(243)).toBe('RISC-V');
    expect(elfProgramTypeLabel(1)).toBe('LOAD');
    expect(elfProgramTypeLabel(6)).toBe('PHDR');
  });

  it('falls back to hex for unknown kinds', () => {
    expect(elfProgramTypeLabel(0xdeadbeef)).toBe('0xdeadbeef');
  });
});
