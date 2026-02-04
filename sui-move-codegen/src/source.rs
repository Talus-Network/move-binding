//! Fetch and normalize Move package metadata using Sui gRPC (`sui-rpc`).

use std::collections::BTreeMap;

use sui_rpc::proto::sui::rpc::v2 as proto;

pub use sui_rpc::Client;
pub use sui_sdk_types::Address;

use crate::ir::{
    Ability, Datatype, DatatypeKind, Field, Function, FunctionParam, NormalizedModule,
    NormalizedPackage, TypeName, TypeParameter, TypeRef, Variant, Visibility,
};
use crate::Error;

/// Fetch a Move package over gRPC and normalize it into [`NormalizedPackage`].
///
/// This is the “online” part of the recommended deterministic pipeline:
/// - fetch once (network),
/// - persist `NormalizedPackage` as JSON (optional),
/// - render Rust bindings from JSON (offline).
///
/// # Example
/// ```rust,no_run
/// use sui_move_codegen::fetch_package;
/// use sui_rpc::Client;
/// use sui_sdk_types::Address;
///
/// # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
/// let mut client = Client::new(Client::MAINNET_FULLNODE)?;
/// let package_id: Address = "0x2".parse()?;
///
/// let pkg = fetch_package(&mut client, package_id).await?;
/// let json = pkg.to_json_string()?;
/// println!("{json}");
/// # Ok(())
/// # }
/// ```
pub async fn fetch_package(
    client: &mut sui_rpc::Client,
    package_id: Address,
) -> Result<NormalizedPackage, Error> {
    let mut package_client = client.package_client();
    let resp = package_client
        .get_package(proto::GetPackageRequest::new(&package_id))
        .await
        .map_err(|e| Error::Rpc(e.to_string()))?
        .into_inner();

    let package = resp.package.ok_or(Error::MissingPackage)?;
    normalize_package(package)
}

fn normalize_package(pkg: proto::Package) -> Result<NormalizedPackage, Error> {
    let storage_id = pkg
        .storage_id
        .as_deref()
        .map(normalize_address)
        .ok_or(Error::MissingField("package.storage_id"))?;
    let version = pkg.version.unwrap_or_default();

    let mut modules_out: BTreeMap<String, NormalizedModule> = BTreeMap::new();
    for module in pkg.modules {
        let m = normalize_module(&module)?;
        modules_out.insert(m.name.clone(), m);
    }

    Ok(NormalizedPackage {
        storage_id,
        original_id: pkg.original_id.as_deref().map(normalize_address),
        version,
        modules: modules_out,
    })
}

fn normalize_module(module: &proto::Module) -> Result<NormalizedModule, Error> {
    let name = module
        .name
        .clone()
        .ok_or(Error::MissingField("module.name"))?;

    let mut datatypes = Vec::new();
    for dt in &module.datatypes {
        datatypes.push(normalize_datatype(dt)?);
    }

    let mut functions = Vec::new();
    for f in &module.functions {
        functions.push(normalize_function(f)?);
    }

    Ok(NormalizedModule {
        name,
        datatypes,
        functions,
    })
}

fn normalize_datatype(dt: &proto::DatatypeDescriptor) -> Result<Datatype, Error> {
    let type_name_str = dt
        .type_name
        .clone()
        .ok_or(Error::MissingField("datatype.type_name"))?;

    let type_name = TypeName::parse(&type_name_str)
        .ok_or_else(|| Error::InvalidTypeName(type_name_str.clone()))?;

    let abilities = dt
        .abilities
        .iter()
        .map(|a| to_ability(*a))
        .collect::<Result<Vec<_>, _>>()?;

    let type_parameters = dt
        .type_parameters
        .iter()
        .map(|tp| {
            let constraints = tp
                .constraints
                .iter()
                .map(|a| to_ability(*a))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(TypeParameter {
                constraints,
                is_phantom: tp.is_phantom.unwrap_or(false),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let raw_kind = dt
        .kind
        .unwrap_or(proto::datatype_descriptor::DatatypeKind::Unknown as i32);
    let kind = match proto::datatype_descriptor::DatatypeKind::try_from(raw_kind) {
        Ok(proto::datatype_descriptor::DatatypeKind::Struct) => {
            let mut fields = dt
                .fields
                .iter()
                .map(normalize_field)
                .collect::<Result<Vec<_>, _>>()?;
            fields.sort_by_key(|f| f.position);
            DatatypeKind::Struct { fields }
        }
        Ok(proto::datatype_descriptor::DatatypeKind::Enum) => {
            let mut variants = dt
                .variants
                .iter()
                .map(normalize_variant)
                .collect::<Result<Vec<_>, _>>()?;
            variants.sort_by_key(|v| v.position);
            DatatypeKind::Enum { variants }
        }
        Ok(proto::datatype_descriptor::DatatypeKind::Unknown) => {
            return Err(Error::UnknownDatatypeKind(raw_kind))
        }
        Ok(_) | Err(_) => return Err(Error::UnknownDatatypeKind(raw_kind)),
    };

    Ok(Datatype {
        module: dt
            .module
            .clone()
            .unwrap_or_else(|| type_name.module.clone()),
        name: dt.name.clone().unwrap_or_else(|| type_name.name.clone()),
        type_name,
        abilities,
        type_parameters,
        kind,
    })
}

fn normalize_field(f: &proto::FieldDescriptor) -> Result<Field, Error> {
    Ok(Field {
        name: f.name.clone().ok_or(Error::MissingField("field.name"))?,
        position: f.position.unwrap_or_default(),
        ty: normalize_type_body(f.r#type.as_ref().ok_or(Error::MissingField("field.type"))?)?,
    })
}

fn normalize_variant(v: &proto::VariantDescriptor) -> Result<Variant, Error> {
    let mut fields = v
        .fields
        .iter()
        .map(normalize_field)
        .collect::<Result<Vec<_>, _>>()?;
    fields.sort_by_key(|f| f.position);

    Ok(Variant {
        name: v.name.clone().ok_or(Error::MissingField("variant.name"))?,
        position: v.position.unwrap_or_default(),
        fields,
    })
}

fn normalize_function(f: &proto::FunctionDescriptor) -> Result<Function, Error> {
    let name = f.name.clone().ok_or(Error::MissingField("function.name"))?;

    let raw_visibility = f
        .visibility
        .unwrap_or(proto::function_descriptor::Visibility::Unknown as i32);
    let visibility = match proto::function_descriptor::Visibility::try_from(raw_visibility) {
        Ok(proto::function_descriptor::Visibility::Public) => Visibility::Public,
        Ok(proto::function_descriptor::Visibility::Friend) => Visibility::Friend,
        Ok(proto::function_descriptor::Visibility::Private) => Visibility::Private,
        Ok(proto::function_descriptor::Visibility::Unknown) => {
            return Err(Error::UnknownVisibility(raw_visibility))
        }
        Ok(_) | Err(_) => return Err(Error::UnknownVisibility(raw_visibility)),
    };

    let type_parameters = f
        .type_parameters
        .iter()
        .map(|tp| {
            let constraints = tp
                .constraints
                .iter()
                .map(|a| to_ability(*a))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(TypeParameter {
                constraints,
                is_phantom: tp.is_phantom.unwrap_or(false),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let parameters = f
        .parameters
        .iter()
        .enumerate()
        .map(|(idx, p)| {
            let ty = normalize_open_signature(p)?;
            Ok(FunctionParam {
                name: format!("arg{idx}"),
                ty,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let return_types = f
        .returns
        .iter()
        .map(normalize_open_signature)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Function {
        name,
        visibility,
        is_entry: f.is_entry.unwrap_or(false),
        type_parameters,
        parameters,
        return_types,
    })
}

fn normalize_open_signature(sig: &proto::OpenSignature) -> Result<TypeRef, Error> {
    let inner = normalize_type_body(
        sig.body
            .as_ref()
            .ok_or(Error::MissingField("signature.body"))?,
    )?;

    match proto::open_signature::Reference::try_from(
        sig.reference
            .unwrap_or(proto::open_signature::Reference::Unknown as i32),
    ) {
        Ok(proto::open_signature::Reference::Immutable) => Ok(TypeRef::Ref {
            mutable: false,
            inner: Box::new(inner),
        }),
        Ok(proto::open_signature::Reference::Mutable) => Ok(TypeRef::Ref {
            mutable: true,
            inner: Box::new(inner),
        }),
        Ok(proto::open_signature::Reference::Unknown) => Ok(inner),
        Ok(_) => Err(Error::UnknownReference(
            sig.reference
                .unwrap_or(proto::open_signature::Reference::Unknown as i32),
        )),
        Err(_) => Err(Error::UnknownReference(
            sig.reference
                .unwrap_or(proto::open_signature::Reference::Unknown as i32),
        )),
    }
}

fn normalize_type_body(sig: &proto::OpenSignatureBody) -> Result<TypeRef, Error> {
    let r#type = sig
        .r#type
        .unwrap_or(proto::open_signature_body::Type::Unknown as i32);

    match proto::open_signature_body::Type::try_from(r#type) {
        Ok(proto::open_signature_body::Type::Address) => Ok(TypeRef::Address),
        Ok(proto::open_signature_body::Type::Bool) => Ok(TypeRef::Bool),
        Ok(proto::open_signature_body::Type::U8) => Ok(TypeRef::U8),
        Ok(proto::open_signature_body::Type::U16) => Ok(TypeRef::U16),
        Ok(proto::open_signature_body::Type::U32) => Ok(TypeRef::U32),
        Ok(proto::open_signature_body::Type::U64) => Ok(TypeRef::U64),
        Ok(proto::open_signature_body::Type::U128) => Ok(TypeRef::U128),
        Ok(proto::open_signature_body::Type::U256) => Ok(TypeRef::U256),
        Ok(proto::open_signature_body::Type::Vector) => {
            let inner = sig
                .type_parameter_instantiation
                .first()
                .ok_or(Error::MissingField("vector.type_parameter_instantiation"))?;
            Ok(TypeRef::Vector(Box::new(normalize_type_body(inner)?)))
        }
        Ok(proto::open_signature_body::Type::Datatype) => {
            let name = sig
                .type_name
                .as_deref()
                .and_then(TypeName::parse)
                .ok_or_else(|| Error::InvalidTypeName(sig.type_name.clone().unwrap_or_default()))?;

            let mut args = Vec::new();
            for t in &sig.type_parameter_instantiation {
                args.push(normalize_type_body(t)?);
            }

            Ok(TypeRef::Datatype {
                type_name: name,
                type_arguments: args,
            })
        }
        Ok(proto::open_signature_body::Type::Parameter) => Ok(TypeRef::TypeParameter(
            sig.type_parameter.unwrap_or_default(),
        )),
        Ok(proto::open_signature_body::Type::Unknown) => Err(Error::MissingField("type")),
        Ok(_) => Err(Error::MissingField("type")),
        Err(_) => Err(Error::MissingField("type")),
    }
}

fn to_ability(a: i32) -> Result<Ability, Error> {
    match proto::Ability::try_from(a) {
        Ok(proto::Ability::Copy) => Ok(Ability::Copy),
        Ok(proto::Ability::Drop) => Ok(Ability::Drop),
        Ok(proto::Ability::Store) => Ok(Ability::Store),
        Ok(proto::Ability::Key) => Ok(Ability::Key),
        _ => Err(Error::UnknownAbility(a)),
    }
}

fn normalize_address(input: &str) -> String {
    let trimmed = input.trim();
    let addr = trimmed
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .trim_start_matches('0');
    let addr = if addr.is_empty() { "0" } else { addr };
    format!("0x{addr}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_open_signature_wraps_refs() {
        let mut body = proto::OpenSignatureBody::default();
        body.r#type = Some(proto::open_signature_body::Type::U64 as i32);

        let mut sig = proto::OpenSignature::default();
        sig.reference = Some(proto::open_signature::Reference::Mutable as i32);
        sig.body = Some(body);

        let out = normalize_open_signature(&sig).unwrap();
        assert!(matches!(out, TypeRef::Ref { mutable: true, .. }));
    }

    #[test]
    fn normalize_package_roundtrip_minimal() {
        let mut uid_body = proto::OpenSignatureBody::default();
        uid_body.r#type = Some(proto::open_signature_body::Type::Datatype as i32);
        uid_body.type_name = Some("0x2::object::UID".into());

        let mut field = proto::FieldDescriptor::default();
        field.name = Some("id".into());
        field.position = Some(0);
        field.r#type = Some(uid_body);

        let mut type_param = proto::TypeParameter::default();
        type_param.constraints = vec![proto::Ability::Store as i32];
        type_param.is_phantom = Some(true);

        let mut datatype = proto::DatatypeDescriptor::default();
        datatype.type_name = Some("0x1::m::S".into());
        datatype.defining_id = Some("0x1".into());
        datatype.module = Some("m".into());
        datatype.name = Some("S".into());
        datatype.abilities = vec![proto::Ability::Store as i32, proto::Ability::Key as i32];
        datatype.type_parameters = vec![type_param];
        datatype.kind = Some(proto::datatype_descriptor::DatatypeKind::Struct as i32);
        datatype.fields = vec![field];

        let mut module = proto::Module::default();
        module.name = Some("m".into());
        module.datatypes = vec![datatype];

        let mut pkg = proto::Package::default();
        pkg.storage_id = Some("0x1".into());
        pkg.original_id = Some("0x1".into());
        pkg.version = Some(7);
        pkg.modules = vec![module];

        let normalized = normalize_package(pkg).unwrap();
        assert_eq!(normalized.storage_id, "0x1");
        let module = normalized.modules.get("m").unwrap();
        let dt = &module.datatypes[0];
        assert_eq!(dt.abilities, vec![Ability::Store, Ability::Key]);
        match &dt.kind {
            DatatypeKind::Struct { fields } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "id");
                match &fields[0].ty {
                    TypeRef::Datatype { type_name, .. } => assert_eq!(type_name.name, "UID"),
                    _ => panic!("unexpected type"),
                }
            }
            _ => panic!("expected struct"),
        }
    }
}
