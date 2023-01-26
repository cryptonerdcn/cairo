use convert_case::Casing;
use itertools::chain;

use super::gas::GasBuiltinType;
use super::range_check::RangeCheckType;
use crate::define_libfunc_hierarchy;
use crate::extensions::lib_func::{
    BranchSignature, DeferredOutputKind, LibfuncSignature, OutputVarInfo, ParamSignature,
    SierraApChange, SignatureSpecializationContext,
};
use crate::extensions::{
    NamedType, NoGenericArgsGenericLibfunc, NoGenericArgsGenericType, OutputVarReferenceInfo,
    SpecializationError,
};
use crate::ids::GenericTypeId;

/// Represents different type of costs.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum CostTokenType {
    /// A single Cairo step, or some cost which is equivalent to it.
    Step,
    /// The cost of adding uninitialized memory cell.
    Hole,
    /// One invocation of the range check builtin.
    RangeCheck,
    /// One invocation of the pedersen hash function.
    Pedersen,
    /// One invocation of the bitwise builtin.
    Bitwise,
    /// One invocation of the EC op builtin.
    EcOp,
}
impl CostTokenType {
    pub fn iter()
    -> std::iter::Chain<std::slice::Iter<'static, Self>, std::slice::Iter<'static, Self>> {
        chain!(Self::iter_precost(), Self::iter_postcost())
    }

    pub fn iter_precost() -> std::slice::Iter<'static, Self> {
        [CostTokenType::Pedersen, CostTokenType::Bitwise, CostTokenType::EcOp].iter()
    }

    pub fn iter_postcost() -> std::slice::Iter<'static, Self> {
        [CostTokenType::Step, CostTokenType::Hole, CostTokenType::RangeCheck].iter()
    }

    /// Returns the name of the token type, in snake_case.
    pub fn name(&self) -> String {
        match self {
            CostTokenType::Step => "step",
            CostTokenType::Hole => "hole",
            CostTokenType::RangeCheck => "range_check",
            CostTokenType::Pedersen => "pedersen",
            CostTokenType::Bitwise => "bitwise",
            CostTokenType::EcOp => "ec_op",
        }
        .into()
    }

    pub fn camel_case_name(&self) -> String {
        self.name().to_case(convert_case::Case::UpperCamel)
    }

    pub fn offset_in_builtin_costs(&self) -> i16 {
        match self {
            CostTokenType::Step | CostTokenType::Hole | CostTokenType::RangeCheck => {
                panic!("offset_in_builtin_costs is not supported for '{}'.", self.camel_case_name())
            }
            CostTokenType::Pedersen => 0,
            CostTokenType::Bitwise => 1,
            CostTokenType::EcOp => 2,
        }
    }
}

/// Represents a pointer to an array with the builtin costs.
/// Every element in the array is the cost of a single invocation of a builtin.
///
/// Offsets to the array are given by [CostTokenType::offset_in_builtin_costs].
#[derive(Default)]
pub struct BuiltinCostsType {}
impl NoGenericArgsGenericType for BuiltinCostsType {
    const ID: GenericTypeId = GenericTypeId::new_inline("BuiltinCosts");
    const STORABLE: bool = true;
    const DUPLICATABLE: bool = true;
    const DROPPABLE: bool = true;
    const SIZE: i16 = 1;
}

define_libfunc_hierarchy! {
    pub enum BuiltinCostLibfunc {
        BuiltinGetGas(BuiltinCostGetGasLibfunc),
        GetBuiltinCosts(BuiltinCostGetBuiltinCostsLibfunc),
    }, BuiltinCostConcreteLibfunc
}

/// Libfunc for getting gas to be used by a builtin.
#[derive(Default)]
pub struct BuiltinCostGetGasLibfunc;

impl NoGenericArgsGenericLibfunc for BuiltinCostGetGasLibfunc {
    const STR_ID: &'static str = "get_gas_all";

    fn specialize_signature(
        &self,
        context: &dyn SignatureSpecializationContext,
    ) -> Result<LibfuncSignature, SpecializationError> {
        let gas_builtin_type = context.get_concrete_type(GasBuiltinType::id(), &[])?;
        let range_check_type = context.get_concrete_type(RangeCheckType::id(), &[])?;
        let builtin_costs_type = context.get_concrete_type(BuiltinCostsType::id(), &[])?;
        Ok(LibfuncSignature {
            param_signatures: vec![
                ParamSignature {
                    ty: range_check_type.clone(),
                    allow_deferred: false,
                    allow_add_const: true,
                    allow_const: false,
                },
                ParamSignature::new(gas_builtin_type.clone()),
                ParamSignature::new(builtin_costs_type),
            ],
            branch_signatures: vec![
                // Success:
                BranchSignature {
                    vars: vec![
                        OutputVarInfo {
                            ty: range_check_type.clone(),
                            ref_info: OutputVarReferenceInfo::Deferred(
                                DeferredOutputKind::AddConst { param_idx: 0 },
                            ),
                        },
                        OutputVarInfo {
                            ty: gas_builtin_type.clone(),
                            ref_info: OutputVarReferenceInfo::NewTempVar { idx: Some(0) },
                        },
                    ],
                    ap_change: SierraApChange::Known { new_vars_only: false },
                },
                // Failure:
                BranchSignature {
                    vars: vec![
                        OutputVarInfo {
                            ty: range_check_type,
                            ref_info: OutputVarReferenceInfo::Deferred(
                                DeferredOutputKind::AddConst { param_idx: 0 },
                            ),
                        },
                        OutputVarInfo {
                            ty: gas_builtin_type,
                            ref_info: OutputVarReferenceInfo::SameAsParam { param_idx: 1 },
                        },
                    ],
                    ap_change: SierraApChange::Known { new_vars_only: false },
                },
            ],
            fallthrough: Some(0),
        })
    }
}

/// Libfunc for getting the pointer to the gas cost array.
/// See [BuiltinCostsType].
#[derive(Default)]
pub struct BuiltinCostGetBuiltinCostsLibfunc {}

impl NoGenericArgsGenericLibfunc for BuiltinCostGetBuiltinCostsLibfunc {
    const STR_ID: &'static str = "get_builtin_costs";

    fn specialize_signature(
        &self,
        context: &dyn SignatureSpecializationContext,
    ) -> Result<LibfuncSignature, SpecializationError> {
        let builtin_costs_type = context.get_concrete_type(BuiltinCostsType::id(), &[])?;
        Ok(LibfuncSignature::new_non_branch(
            vec![],
            vec![OutputVarInfo {
                ty: builtin_costs_type,
                ref_info: OutputVarReferenceInfo::Deferred(DeferredOutputKind::Generic),
            }],
            SierraApChange::Known { new_vars_only: false },
        ))
    }
}
