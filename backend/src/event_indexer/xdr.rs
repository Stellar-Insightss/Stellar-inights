use std::{collections::BTreeSet, str};

use stellar_xdr::{ContractEvent, ContractEventBody, ContractEventType, ScMap, ScMapEntry, ScVal};

use super::dispatch::DispatchError;

pub(crate) struct DecodedEvent<'a> {
    pub contract_id: String,
    pub topic: String,
    pub data: &'a ScMap,
}

pub(crate) fn decode_event(event: &ContractEvent) -> Result<DecodedEvent<'_>, DispatchError> {
    if event.type_ != ContractEventType::Contract {
        return Err(DispatchError::UnsupportedEventType(event.type_.to_string()));
    }

    let contract_id = event
        .contract_id
        .as_ref()
        .ok_or(DispatchError::MissingContractId)?
        .to_string();

    let ContractEventBody::V0(body) = &event.body;
    if body.topics.len() != 1 {
        return Err(DispatchError::InvalidTopicCount(body.topics.len()));
    }

    let ScVal::Symbol(topic) = &body.topics[0] else {
        return Err(DispatchError::InvalidTopicType);
    };
    let topic = str::from_utf8(topic.as_ref())
        .map_err(|_| DispatchError::InvalidTopicEncoding)?
        .to_owned();

    let ScVal::Map(Some(data)) = &body.data else {
        return Err(DispatchError::InvalidEventData);
    };

    Ok(DecodedEvent {
        contract_id,
        topic,
        data,
    })
}

pub(crate) fn validate_fields(
    data: &ScMap,
    expected: &'static [&'static str],
    schema_version: u32,
) -> Result<(), DispatchError> {
    let mut seen = BTreeSet::new();

    for entry in data.0.iter() {
        let name = field_name(entry)?;
        if !seen.insert(name.to_owned()) {
            return Err(DispatchError::DuplicateField(name.to_owned()));
        }
        if !expected.contains(&name) {
            return Err(DispatchError::UnexpectedField {
                field: name.to_owned(),
                schema_version,
            });
        }
    }

    for field in expected {
        if !seen.contains(*field) {
            return Err(DispatchError::MissingField(field));
        }
    }

    Ok(())
}

pub(crate) fn u32_field(data: &ScMap, name: &'static str) -> Result<u32, DispatchError> {
    match field(data, name)? {
        ScVal::U32(value) => Ok(*value),
        _ => Err(DispatchError::InvalidFieldType {
            field: name,
            expected: "an XDR u32",
        }),
    }
}

pub(crate) fn u64_field(data: &ScMap, name: &'static str) -> Result<u64, DispatchError> {
    match field(data, name)? {
        ScVal::U64(value) => Ok(*value),
        _ => Err(DispatchError::InvalidFieldType {
            field: name,
            expected: "an XDR u64",
        }),
    }
}

pub(crate) fn bytes32_field(data: &ScMap, name: &'static str) -> Result<String, DispatchError> {
    match field(data, name)? {
        ScVal::Bytes(bytes) if bytes.0.len() == 32 => Ok(hex::encode(bytes.0.as_slice())),
        _ => Err(DispatchError::InvalidFieldType {
            field: name,
            expected: "exactly 32 XDR bytes",
        }),
    }
}

pub(crate) fn address_field(data: &ScMap, name: &'static str) -> Result<String, DispatchError> {
    match field(data, name)? {
        ScVal::Address(address) => Ok(address.to_string()),
        _ => Err(DispatchError::InvalidFieldType {
            field: name,
            expected: "an XDR address",
        }),
    }
}

fn field<'a>(data: &'a ScMap, name: &'static str) -> Result<&'a ScVal, DispatchError> {
    for entry in data.0.iter() {
        if field_name(entry)? == name {
            return Ok(&entry.val);
        }
    }
    Err(DispatchError::MissingField(name))
}

fn field_name(entry: &ScMapEntry) -> Result<&str, DispatchError> {
    let ScVal::Symbol(symbol) = &entry.key else {
        return Err(DispatchError::InvalidFieldNameType);
    };
    str::from_utf8(symbol.as_ref()).map_err(|_| DispatchError::InvalidFieldNameEncoding)
}
