use std::fmt::Display;

use crate::instructions::{AssertEqInstruction, Instruction, JumpInstruction};
use crate::operand::{DerefOperand, ImmediateOperand, Register, ResOperand};

#[test]
fn test_jump_format() {
    let abs_jmp_insn = JumpInstruction {
        target: ResOperand::ImmediateOperand(ImmediateOperand { value: 3 }),
        relative: false,
    };

    assert_eq!(abs_jmp_insn.to_string(), "jmp abs 3");

    let rel_jmp_insn: Instruction = Instruction::JumpInstruction(JumpInstruction {
        target: ResOperand::ImmediateOperand(ImmediateOperand { value: -5 }),
        relative: true,
    });

    assert_eq!(rel_jmp_insn.to_string(), "jmp rel -5");
}

#[test]
fn test_assert_eq_format() {
    let op1 = DerefOperand { register: Register::AP, offset: 5 };
    let op2 = ResOperand::ImmediateOperand(ImmediateOperand { value: 205 });

    let insn = AssertEqInstruction { a: op1, b: op2 };
    assert_eq!(insn.to_string(), "[ap + 5] = 205");
}
