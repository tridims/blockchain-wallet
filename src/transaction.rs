//! Module defining Ethereum transaction data as well as an RLP encoding
//! implementation.

pub mod accesslist;
mod rlp;

use crate::utils::hash;
use crate::{transaction::accesslist::AccessList, utils::serialization, wallet::Signature};
use anyhow::Result;
use ethaddr::Address;
use ethnum::U256;
use serde::Deserialize;

/// An EIP-1559 Ethereum transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    /// The chain ID for the transaction.
    #[serde(with = "ethnum::serde::permissive")]
    pub chain_id: U256,

    /// The nonce for the transaction.
    #[serde(with = "ethnum::serde::permissive")]
    pub nonce: U256,

    /// The maximum priority fee in Wei for the transaction.
    #[serde(with = "ethnum::serde::permissive")]
    pub max_priority_fee_per_gas: U256,

    /// The maximum gas price in Wei for the transaction.
    #[serde(with = "ethnum::serde::permissive")]
    pub max_fee_per_gas: U256,

    /// The gas limit for the transaction.
    #[serde(with = "ethnum::serde::permissive")]
    pub gas: U256,

    /// The target address for the transaction. This can also be `None` to
    /// indicate a contract creation transaction.
    pub to: Option<Address>,

    /// The amount of Ether to send with the transaction.
    #[serde(with = "ethnum::serde::permissive")]
    pub value: U256,

    /// The calldata to use for the transaction.
    #[serde(with = "serialization::bytes")]
    pub data: Vec<u8>,

    /// List of addresses and storage keys that the transaction plans to access.
    #[serde(default)]
    pub access_list: AccessList,
}

impl Transaction {
    // Sign with a wallet.
    pub fn sign_with_wallet(&mut self, wallet: &crate::wallet::Wallet) -> Result<Vec<u8>> {
        let message = self.get_unsigned_rlp_encoded();
        let signature = wallet.sign(message)?;
        let encoded = self.get_signed_rlp_encoded(signature);

        Ok(encoded)
    }

    /// Returns the RLP encoded transaction without signature.
    pub fn get_unsigned_rlp_encoded(&self) -> [u8; 32] {
        hash::keccak256(self.rlp_encode(None))
    }

    /// Returns 32-byte message used for signing.
    pub fn get_signed_rlp_encoded(&self, signature: Signature) -> Vec<u8> {
        self.rlp_encode(Some(signature))
    }

    /// Returns the RLP encoded transaction with an optional signature.
    pub fn rlp_encode(&self, signature: Option<Signature>) -> Vec<u8> {
        let fields = [
            rlp::uint(self.chain_id),
            rlp::uint(self.nonce),
            rlp::uint(self.max_priority_fee_per_gas),
            rlp::uint(self.max_fee_per_gas),
            rlp::uint(self.gas),
            self.to
                .map_or_else(|| rlp::bytes(b""), |to| rlp::bytes(&*to)),
            rlp::uint(self.value),
            rlp::bytes(&self.data),
            self.access_list.rlp_encode(),
        ];

        let tail = signature.map(|signature| {
            [
                rlp::uint(signature.y_parity()),
                rlp::uint(signature.r()),
                rlp::uint(signature.s()),
            ]
        });

        // Add the header for EIP-1559 transactions. Based on EIP-2718.
        [
            &[0x02][..],
            &rlp::iter(fields.iter().chain(tail.iter().flatten())),
        ]
        .concat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::accesslist::StorageSlot;
    use crate::wallet::Wallet;
    use ethaddr::address;
    use ethnum::AsU256 as _;
    use hex_literal::hex;
    use serde_json::{json, Value};

    pub const DETERMINISTIC_PRIVATE_KEY: [u8; 32] =
        hex!("4f3edf983ac636a65a842ce7c78d9aa706d3b113bce9c46f30d7d21715b23b1d");

    fn sign_encode(tx: Value) -> Vec<u8> {
        let tx = serde_json::from_value::<Transaction>(tx).unwrap();
        let wallet = Wallet::from_secret(DETERMINISTIC_PRIVATE_KEY).unwrap();
        let signature = wallet.sign(tx.get_unsigned_rlp_encoded()).unwrap();
        tx.get_signed_rlp_encoded(signature)
    }

    #[test]
    fn encode_signed_transaction() {
        assert_eq!(
            sign_encode(json!({
                "chainId": 1,
                "nonce": 0,
                "maxPriorityFeePerGas": 0,
                "maxFeePerGas": 0,
                "gas": 21000,
                "to": "0x0000000000000000000000000000000000000000",
                "value": 0,
                "data": "0x",
            })),
            hex!(
                "02f8620180808082520894000000000000000000000000000000000000000080
                 80c001a0290dbdecbc884b4cb827015fe0cd7ac90df1a5634d52a2845c21afac
                 ca14b803a03e848dd1a342e5528beff99c42876cf091a68e2090dbbced5a5f7f
                 392d3abcda"
            ),
        );
    }

    #[test]
    fn deserialize_json() {
        let mut tx = json!({
            "chainId": "0xff",
            "nonce": 42,
            "maxPriorityFeePerGas": 13.37e9,
            "maxFeePerGas": 42e9,
            "gas": 21000,
            "value": "13370000000000000000",
            "data": "0x",
        });
        assert_eq!(
            serde_json::from_value::<Transaction>(tx.clone()).unwrap(),
            Transaction {
                chain_id: 255.as_u256(),
                nonce: 42.as_u256(),
                max_priority_fee_per_gas: 13.37e9.as_u256(),
                max_fee_per_gas: 42e9.as_u256(),
                gas: 21_000.as_u256(),
                to: None,
                value: 13.37e18.as_u256(),
                data: vec![],
                access_list: AccessList::default(),
            }
        );

        tx["to"] = json!("0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
        tx["accessList"] = json!([[
            "0x0000000000000000000000000000000000000000",
            ["0x0000000000000000000000000000000000000000000000000000000000000000",],
        ]]);
        let deserialized = serde_json::from_value::<Transaction>(tx).unwrap();
        assert_eq!(
            deserialized.to.unwrap(),
            address!("0xDeaDbeefdEAdbeefdEadbEEFdeadbeEFdEaDbeeF"),
        );
        assert_eq!(
            deserialized.access_list,
            AccessList(vec![(Address::default(), vec![StorageSlot::default()])]),
        );
    }

    #[test]
    fn encode() {
        assert_eq!(
            Transaction {
                chain_id: 1.as_u256(),
                nonce: 66.as_u256(),
                max_priority_fee_per_gas: 28e9.as_u256(),
                max_fee_per_gas: 42e9.as_u256(),
                gas: 30_000.as_u256(),
                to: Some(address!("0xDeaDbeefdEAdbeefdEadbEEFdeadbeEFdEaDbeeF")),
                value: 13.37e18.as_u256(),
                data: vec![],
                access_list: AccessList::default(),
            }
            .rlp_encode(None),
            hex!(
                "02f10142850684ee18008509c765240082753094deadbeefdeadbeefdeadbeefdeadbeefde
                 adbeef88b98bc829a6f9000080c0"
            )
            .to_owned(),
        );
        assert_eq!(
            Transaction {
                chain_id: 1.as_u256(),
                nonce: 777.as_u256(),
                max_priority_fee_per_gas: 28e9.as_u256(),
                max_fee_per_gas: 42e9.as_u256(),
                gas: 100_000.as_u256(),
                to: None,
                value: 0.as_u256(),
                data: hex!(
                    "363d3d373d3d3d363d73deadbeefdeadbeefdeadbeefdeadbeefdeadbeef5af43d82803e90
                     3d91602b57fd5bf3"
                )
                .to_vec(),
                access_list: AccessList(vec![
                    (
                        address!("0x1111111111111111111111111111111111111111"),
                        vec![
                            StorageSlot(hex!(
                                "a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0"
                            )),
                            StorageSlot(hex!(
                                "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
                            )),
                        ],
                    ),
                    (
                        address!("0x2222222222222222222222222222222222222222"),
                        vec![],
                    ),
                ]),
            }
            .rlp_encode(None),
            hex!(
                "02f8b801820309850684ee18008509c7652400830186a08080ad363d3d373d3d
                 3d363d73deadbeefdeadbeefdeadbeefdeadbeefdeadbeef5af43d82803e903d
                 91602b57fd5bf3f872f859941111111111111111111111111111111111111111
                 f842a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0
                 a0a0a0a0a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1
                 a1a1a1a1d6942222222222222222222222222222222222222222c0"
            )
            .to_vec(),
        );
    }
}
