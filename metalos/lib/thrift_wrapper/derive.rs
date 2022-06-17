/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::spanned::Spanned;
use syn::NestedMeta::Lit;
use syn::{parse_macro_input, Data, DataEnum, DeriveInput, Error, Fields, Meta, MetaList};

fn get_fields(input: DeriveInput) -> syn::Result<syn::FieldsNamed> {
    let span = input.span();
    match input.data {
        Data::Struct(s) => match s.fields {
            Fields::Named(n) => Ok(n),
            _ => Err(Error::new(
                span,
                "only structs with named fields are supported",
            )),
        },
        _ => Err(Error::new(span, "only structs are supported")),
    }
}

fn expand_thriftwrapper_struct(
    thrift_type: syn::Type,
    name: syn::Ident,
    fields: syn::FieldsNamed,
) -> proc_macro2::TokenStream {
    let mut field_idents = vec![];
    let mut fields_from_thrift = vec![];
    let mut fields_into_thrift = vec![];
    for f in &fields.named {
        let ty = &f.ty;
        let id = f.ident.as_ref().expect("fields are named");
        field_idents.push(id);
        fields_from_thrift.push(quote! {
            #id: <#ty as ::thrift_wrapper::ThriftWrapper>::from_thrift(#id).in_field(stringify!(#id))?,
        });
        fields_into_thrift.push(quote! {
            #id: <#ty as ::thrift_wrapper::ThriftWrapper>::into_thrift(self.#id),
        });
    }
    quote! {
        impl ::thrift_wrapper::ThriftWrapper for #name {
            type Thrift = #thrift_type;

            fn from_thrift(t: #thrift_type) -> ::thrift_wrapper::Result<Self> {
                let #thrift_type {
                    #(#field_idents),*
                } = t;
                use ::thrift_wrapper::{FieldContext, ThriftWrapper};
                Ok(Self {
                    #(#fields_from_thrift)*
                })
            }

            fn into_thrift(self) -> #thrift_type {
                use ::thrift_wrapper::ThriftWrapper;
                #thrift_type {
                    #(#fields_into_thrift)*
                }
            }
        }
    }
}

fn get_meta_str(meta: MetaList) -> syn::Result<String> {
    if meta.nested.len() != 1 {
        return Err(Error::new(
            meta.span(),
            "thrift_field_name must have only one value",
        ));
    }

    let meta_field_opt = meta.nested.first();
    match meta_field_opt {
        Some(meta_field) => match meta_field {
            Lit(syn::Lit::Str(litstr)) => Ok(litstr.value()),
            _ => Err(Error::new(
                meta_field.span(),
                "thrift_field_name must be a string",
            )),
        },
        None => Err(Error::new(
            meta_field_opt.span(),
            "thrift_field_name must have a value",
        )),
    }
}

fn get_overriden_name(attrs: &Vec<syn::Attribute>) -> syn::Result<Option<String>> {
    for attr in attrs {
        if attr.path.is_ident("thrift_field_name") {
            return match attr.parse_meta() {
                Ok(Meta::List(meta)) => get_meta_str(meta).map(|x| Some(x)),
                e => Err(Error::new(
                    attr.span(),
                    format!("thrift_field_name must have a value: {:?}", e),
                )),
            };
        }
    }
    Ok(None)
}

fn expand_thriftwrapper_enum(
    thrift_type: syn::Type,
    name: syn::Ident,
    enm: &DataEnum,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut match_thrift_variant = vec![];
    let mut match_to_thrift = vec![];
    let mut rm_rust_variant_from_thrift_set = vec![];
    let mut is_union = false;
    for var in &enm.variants {
        let rust_name = &var.ident;
        // with no variant, it is a thrift enum (aka, constant value)
        if var.fields.is_empty() {
            let thrift_name = match get_overriden_name(&var.attrs)? {
                Some(v) => format_ident!("{}", v),
                None => format_ident!("{}", var.ident.to_string().to_case(Case::UpperSnake)),
            };

            match_thrift_variant.push(quote! {
                #thrift_type::#thrift_name => Ok(Self::#rust_name),
            });
            match_to_thrift.push(quote! {
                Self::#rust_name => #thrift_type::#thrift_name,
            });
            rm_rust_variant_from_thrift_set.push(quote! {
                thrift_variants.remove(&#thrift_type::#thrift_name);
            });
        }
        // there's a variant so it's a union field
        else {
            is_union = true;
            let thrift_name = match get_overriden_name(&var.attrs)? {
                Some(v) => format_ident!("{}", v),
                None => format_ident!("{}", var.ident.to_string().to_case(Case::Snake)),
            };
            match_thrift_variant.push(quote! {
                #thrift_type::#thrift_name(x) => {
                    ::thrift_wrapper::ThriftWrapper::from_thrift(x)
                        .map(Self::#rust_name)
                }
            });
            match_to_thrift.push(quote! {
                Self::#rust_name(x) => #thrift_type::#thrift_name(::thrift_wrapper::ThriftWrapper::into_thrift(x)),
            });
        }
    }
    let unknown_variant_from_thrift = match is_union {
        false => quote! {
            _ => Err(::thrift_wrapper::Error::Enum(format!("{:?}", t))),
        },
        true => quote! {
            #thrift_type::UnknownField(i) => Err(::thrift_wrapper::Error::Union(i)),
        },
    };
    let test_enum_variants = match is_union {
        true => quote!(),
        false => {
            // create a unit test that will fail if the rust version doesn't have all
            // the variants that are defined in thrift
            let test_enum_variants_name = format_ident!("rust_{}_has_all_variants", name);
            quote! {
                #[cfg(test)]
                #[test]
                #[allow(non_snake_case)]
                fn #test_enum_variants_name () {
                    let mut thrift_variants = ::std::collections::HashSet::new();
                    for kind in <#thrift_type as ::thrift_wrapper::__deps::fbthrift::ThriftEnum>::variant_values() {
                        thrift_variants.insert(kind);
                    }
                    #(#rm_rust_variant_from_thrift_set)*
                    assert!(thrift_variants.is_empty(), "Rust version is missing {:?}", thrift_variants);
                }
            }
        }
    };
    Ok(quote! {
        #test_enum_variants

        impl ::thrift_wrapper::ThriftWrapper for #name {
            type Thrift = #thrift_type;

            fn from_thrift(t: #thrift_type) -> ::thrift_wrapper::Result<Self> {
                match t {
                    #(#match_thrift_variant)*
                    #unknown_variant_from_thrift
                }
            }

            fn into_thrift(self) -> #thrift_type {
                match self {
                    #(#match_to_thrift)*
                }
            }
        }
    })
}

fn expand_thriftwrapper_newtype_struct(
    thrift_type: syn::Type,
    name: syn::Ident,
) -> proc_macro2::TokenStream {
    quote! {
        impl ::thrift_wrapper::ThriftWrapper for #name {
            type Thrift = #thrift_type;

            fn from_thrift(t: #thrift_type) -> ::thrift_wrapper::Result<Self> {
                <#thrift_type as ::thrift_wrapper::ThriftWrapper>::from_thrift(t).map(Self)
            }

            fn into_thrift(self) -> #thrift_type {
                self.0.into()
            }
        }
    }
}

fn get_thrift_type(input: DeriveInput) -> syn::Result<syn::Type> {
    let thrift_attr = input
        .attrs
        .iter()
        .find(|a| a.path.is_ident("thrift"))
        .ok_or_else(|| {
            Error::new(
                input.span(),
                "must have thrift attribute pointing to the thrift type",
            )
        })?;
    match thrift_attr.parse_meta()? {
        Meta::List(mut lst) => {
            if lst.nested.len() != 1 {
                return Err(Error::new(
                    lst.span(),
                    "thrift attr must be of the form 'thrift(type)'",
                ));
            }
            let only = lst.nested.pop().unwrap();
            syn::parse2(only.to_token_stream())
        }
        _ => Err(Error::new(
            thrift_attr.span(),
            "thrift attr must be of the form 'thrift(type)'",
        )),
    }
}

fn common_impls(thrift_type: syn::Type, name: syn::Ident) -> proc_macro2::TokenStream {
    quote! {
        impl ::std::convert::TryFrom<#thrift_type> for #name {
            type Error = ::thrift_wrapper::Error;

            fn try_from(t: #thrift_type) -> ::std::result::Result<Self, Self::Error> {
                ::thrift_wrapper::ThriftWrapper::from_thrift(t)
            }
        }

        impl ::std::convert::From<#name> for #thrift_type {
            fn from(t: #name) -> Self {
                ::thrift_wrapper::ThriftWrapper::into_thrift(t)
            }
        }

        impl<P> ::thrift_wrapper::__deps::fbthrift::Deserialize<P> for #name
        where
            #thrift_type: ::thrift_wrapper::__deps::fbthrift::Deserialize<P>,
            P: ::thrift_wrapper::__deps::fbthrift::ProtocolReader,
        {
            fn read(p: &mut P) -> ::thrift_wrapper::__deps::anyhow::Result<Self>
            where
                Self: Sized,
            {
                let thrift = #thrift_type::read(p)?;
                use ::thrift_wrapper::__deps::anyhow::Context;
                thrift
                    .try_into()
                    .context("while converting deserialized thrift into rust representation")
            }
        }

        impl<P> ::thrift_wrapper::__deps::fbthrift::Serialize<P> for #name
        where
            #thrift_type: ::thrift_wrapper::__deps::fbthrift::Serialize<P>,
            P: ::thrift_wrapper::__deps::fbthrift::ProtocolWriter,
        {
            fn write(&self, p: &mut P) {
                let owned_self: #name = self.clone();
                ::thrift_wrapper::ThriftWrapper::into_thrift(owned_self).write(p)
            }
        }

        impl std::default::Default for #name {
            fn default() -> Self {
                <#thrift_type>::default().try_into().expect("Default::default must be convertible")
            }
        }
    }
}

fn expand_thriftwrapper(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = input.ident.clone();
    let span = input.span();

    if let Data::Struct(syn::DataStruct {
        fields: Fields::Unnamed(ref tuple_struct_fields),
        semi_token: _,
        struct_token: _,
    }) = input.data
    {
        if tuple_struct_fields.unnamed.len() != 1 {
            return Err(Error::new(
                tuple_struct_fields.span(),
                "newtype struct must have exactly one field",
            ));
        }
        return Ok(expand_thriftwrapper_newtype_struct(
            tuple_struct_fields.unnamed.first().unwrap().ty.clone(),
            name,
        ));
    }

    let thrift_type = get_thrift_type(input.clone())?;

    let specialization = match &input.data {
        Data::Struct(_) => {
            let fields = get_fields(input)?;
            Ok(expand_thriftwrapper_struct(
                thrift_type.clone(),
                name.clone(),
                fields,
            ))
        }
        Data::Enum(enm) => expand_thriftwrapper_enum(thrift_type.clone(), name.clone(), enm),
        _ => Err(Error::new(span, "only structs/enums are supported")),
    }?;

    let common = common_impls(thrift_type, name);

    Ok(quote! {
        #specialization
        #common
    })
}

#[proc_macro_derive(ThriftWrapper, attributes(thrift, thrift_field_name))]
pub fn derive_thriftwrapper(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_thriftwrapper(input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}
