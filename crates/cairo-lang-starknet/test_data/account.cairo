use serde::Serde;
use starknet::ContractAddress;
use array::ArrayTrait;
use array::SpanTrait;
use option::OptionTrait;

#[account_contract]
mod Account {
    use array::ArrayTrait;
    use array::SpanTrait;
    use box::BoxTrait;
    use ecdsa::check_ecdsa_signature;
    use option::OptionTrait;
    use super::Call;
    use starknet::ContractAddress;
    use zeroable::Zeroable;

    struct Storage {
        public_key: felt252
    }

    #[constructor]
    fn constructor(public_key_: felt252) {
        public_key::write(public_key_);
    }

    fn validate_transaction() -> felt252 {
        let tx_info = starknet::get_tx_info().unbox();
        let signature = tx_info.signature;
        assert(signature.len() == 2_u32, 'INVALID_SIGNATURE_LENGTH');
        assert(
            check_ecdsa_signature(
                message_hash: tx_info.transaction_hash,
                public_key: public_key::read(),
                signature_r: *signature[0_u32],
                signature_s: *signature[1_u32],
            ),
            'INVALID_SIGNATURE',
        );

        starknet::VALIDATED
    }


    #[external]
    fn __validate_deploy__(
        class_hash: felt252, contract_address_salt: felt252, public_key_: felt252
    ) -> felt252 {
        validate_transaction()
    }

    #[external]
    fn __validate_declare__(class_hash: felt252) -> felt252 {
        validate_transaction()
    }

    #[external]
    fn __validate__(
        contract_address: ContractAddress, entry_point_selector: felt252, calldata: Array<felt252>
    ) -> felt252 {
        validate_transaction()
    }

    #[external]
    #[raw_output]
    fn __execute__(mut calls: Array<Call>) -> Span<felt252> {
        // Validate caller.
        assert(starknet::get_caller_address().is_zero(), 'INVALID_CALLER');

        // Check the tx version here, since version 0 transaction skip the __validate__ function.
        let tx_info = starknet::get_tx_info().unbox();
        assert(tx_info.version != 0, 'INVALID_TX_VERSION');

        // TODO(ilya): Implement multi call.
        assert(calls.len() == 1_u32, 'MULTI_CALL_NOT_SUPPORTED');
        let Call{to, selector, calldata } = calls.pop_front().unwrap();

        starknet::call_contract_syscall(
            address: to, entry_point_selector: selector, calldata: calldata.span()
        ).unwrap_syscall()
    }
}

#[derive(Drop)]
struct Call {
    to: ContractAddress,
    selector: felt252,
    calldata: Array<felt252>
}

impl CallSerde of Serde<Call> {
    fn serialize(self: @Call, ref output: Array<felt252>) {
        let Call{to, selector, calldata } = self;
        to.serialize(ref output);
        selector.serialize(ref output);
        calldata.serialize(ref output);
    }

    fn deserialize(ref serialized: Span<felt252>) -> Option<Call> {
        let to = Serde::<ContractAddress>::deserialize(ref serialized)?;
        let selector = Serde::<felt252>::deserialize(ref serialized)?;
        let calldata = Serde::<Array<felt252>>::deserialize(ref serialized)?;
        Option::Some(Call { to, selector, calldata })
    }
}

fn serialize_array_call_helper(ref output: Array<felt252>, mut input: Array<Call>) {
    gas::withdraw_gas().expect('Out of gas');
    match input.pop_front() {
        Option::Some(value) => {
            value.serialize(ref output);
            serialize_array_call_helper(ref output, input);
        },
        Option::None(_) => {},
    }
}

fn deserialize_array_call_helper(
    ref serialized: Span<felt252>, mut curr_output: Array<Call>, remaining: felt252
) -> Option<Array<Call>> {
    if remaining == 0 {
        return Option::Some(curr_output);
    }

    gas::withdraw_gas().expect('Out of gas');

    curr_output.append(Serde::<Call>::deserialize(ref serialized)?);
    deserialize_array_call_helper(ref serialized, curr_output, remaining - 1)
}
