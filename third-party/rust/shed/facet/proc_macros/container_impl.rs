/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{parse_macro_input, Attribute, Error, Expr, Fields, Ident, ItemStruct, Token, Type};

use crate::facet_crate_name;

#[derive(Debug)]
struct ContainerMembers {
    field_idents: Vec<Ident>,
    field_inits: Vec<Expr>,
    facet_idents: Vec<Ident>,
    facet_types: Vec<Type>,
    delegate_idents: Vec<Ident>,
    delegate_types: Vec<Type>,
    delegate_facets: Vec<Vec<Type>>,
}

impl ContainerMembers {
    fn extract(container: &mut ItemStruct) -> Result<Self, Error> {
        let mut field_idents = Vec::new();
        let mut field_inits = Vec::new();
        let mut facet_idents = Vec::new();
        let mut facet_types = Vec::new();
        let mut delegate_idents = Vec::new();
        let mut delegate_types = Vec::new();
        let mut delegate_facets = Vec::new();
        match &mut container.fields {
            Fields::Named(named_fields) => {
                for field in named_fields.named.iter_mut() {
                    let mut attr_found = false;
                    let mut new_attrs = Vec::new();
                    for attr in field.attrs.drain(..) {
                        if attr.path.is_ident("init") {
                            if attr_found {
                                return Err(Error::new(
                                    attr.span(),
                                    concat!(
                                        "facet::container field must have exactly one ",
                                        "of 'init', 'facet' or 'delegate', found multiple"
                                    ),
                                ));
                            }
                            attr_found = true;
                            let expr: Expr = attr.parse_args()?;
                            field_idents
                                .push(field.ident.clone().expect("named field must have a name"));
                            field_inits.push(expr);
                        } else if attr.path.is_ident("facet") {
                            if attr_found {
                                return Err(Error::new(
                                    attr.span(),
                                    concat!(
                                        "facet::container field must have exactly one ",
                                        "of 'init', 'facet' or 'delegate', found multiple"
                                    ),
                                ));
                            }
                            attr_found = true;
                            let mut facet_type = field.ty.clone();
                            if let Type::TraitObject(obj) = &mut facet_type {
                                obj.bounds.push(syn::parse2(quote!(::std::marker::Send))?);
                                obj.bounds.push(syn::parse2(quote!(::std::marker::Sync))?);
                                obj.bounds.push(syn::parse2(quote!('static))?);
                            }
                            field.ty = syn::parse2(quote!(::std::sync::Arc<#facet_type>))?;
                            facet_idents
                                .push(field.ident.clone().expect("named field must have a name"));
                            facet_types.push(facet_type);
                        } else if attr.path.is_ident("delegate") {
                            if attr_found {
                                return Err(Error::new(
                                    attr.span(),
                                    concat!(
                                        "facet::container field must have exactly one ",
                                        "of 'init', 'facet' or 'delegate', found multiple"
                                    ),
                                ));
                            }
                            attr_found = true;
                            let delegate_type = field.ty.clone();
                            let facets = extract_delegate_facets(&attr)?;
                            delegate_idents
                                .push(field.ident.clone().expect("named field must have a name"));
                            delegate_types.push(delegate_type);
                            delegate_facets.push(facets);
                        } else {
                            new_attrs.push(attr);
                        }
                    }
                    if !attr_found {
                        return Err(Error::new(
                            field.span(),
                            concat!(
                                "facet::container field must have exactly one ",
                                "of 'init', 'facet' or 'delegate', found none"
                            ),
                        ));
                    }
                    field.attrs = new_attrs;
                }
            }
            _ => {
                return Err(Error::new(
                    container.ident.span(),
                    "facet::container requires a struct with named fields",
                ));
            }
        }

        Ok(ContainerMembers {
            field_idents,
            field_inits,
            facet_idents,
            facet_types,
            delegate_idents,
            delegate_types,
            delegate_facets,
        })
    }
}

pub fn container(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let _ = parse_macro_input!(attr as syn::parse::Nothing);
    let container = parse_macro_input!(item as ItemStruct);

    match gen_container(container) {
        Ok(output) => output,
        Err(e) => e.to_compile_error(),
    }
    .into()
}

fn gen_container(mut container: ItemStruct) -> Result<TokenStream, Error> {
    let facet_crate = format_ident!("{}", facet_crate_name());
    let members = ContainerMembers::extract(&mut container)?;
    let container_name = &container.ident;

    let attr_impls = gen_attr_impls(&facet_crate, container_name, &members);
    let buildable_impl = gen_buildable_impl(&facet_crate, &container_name, &members);
    let async_buildable_impl = gen_async_buildable_impl(&facet_crate, &container_name, &members);

    Ok(quote! {
        #container

        #( #attr_impls )*

        #buildable_impl

        #async_buildable_impl
    })
}

fn gen_buildable_impl(
    facet_crate: &Ident,
    container_name: &Ident,
    members: &ContainerMembers,
) -> TokenStream {
    let facet_idents = &members.facet_idents;
    let facet_types = &members.facet_types;
    let field_idents = &members.field_idents;
    let field_inits = &members.field_inits;
    let delegate_idents = &members.delegate_idents;
    let delegate_types = &members.delegate_types;

    quote! {
        impl<B> ::#facet_crate::Buildable<B> for #container_name
        where B: ::std::marker::Send + ::std::marker::Sync
            #( + ::#facet_crate::Builder<::std::sync::Arc<#facet_types>> )*,
            #( #delegate_types: ::#facet_crate::Buildable<B>, )*
        {
           fn build(builder: &mut B) -> ::std::result::Result<Self, ::#facet_crate::FactoryError> {

                // Build each delegate.
                #(
                    let #delegate_idents =
                        <#delegate_types as ::#facet_crate::Buildable<B>>::build(builder)?;
                )*

                // Build each facet.
                #(
                    let #facet_idents =
                        <B as ::#facet_crate::Builder<
                            ::std::sync::Arc<#facet_types>
                        >>::build(builder)?;
                )*

                // Initialize the other fields.
                #(
                    let #field_idents = #field_inits;
                )*

                Ok(Self {
                    #( #delegate_idents, )*
                    #( #field_idents, )*
                    #( #facet_idents, )*
                })
           }
        }
    }
}

fn gen_async_buildable_impl(
    facet_crate: &Ident,
    container_name: &Ident,
    members: &ContainerMembers,
) -> TokenStream {
    let facet_idents = &members.facet_idents;
    let facet_types = &members.facet_types;
    let field_idents = &members.field_idents;
    let field_inits = &members.field_inits;
    let delegate_idents = &members.delegate_idents;
    let delegate_types = &members.delegate_types;

    // Desugared async-trait so that the builder lifetime can be specified.
    quote! {
        impl<'builder, B> ::#facet_crate::AsyncBuildable<'builder, B> for #container_name
        where B: ::std::marker::Send + ::std::marker::Sync + ::#facet_crate::AsyncBuilder
            #( + ::#facet_crate::AsyncBuilderFor<::std::sync::Arc<#facet_types>> )*
            + 'builder,
            #( #delegate_types: ::#facet_crate::AsyncBuildable<'builder, B>, )*
        {
            fn build_async(mut builder: B) -> ::std::pin::Pin<::std::boxed::Box<
                dyn std::future::Future<
                    Output = ::std::result::Result<Self, ::#facet_crate::FactoryError>
                > + ::std::marker::Send + 'builder
            >>
            {
                let build = async move {
                    // Mark needed facets as needed.
                    Self::mark_needed(&mut builder);

                    // Build the needed facets.
                    <B as ::#facet_crate::AsyncBuilder>::build_needed(&mut builder).await?;

                    // Build ourself.
                    Ok(Self::construct(&builder))

                };
                ::std::boxed::Box::pin(build)
           }

           fn mark_needed(builder: &mut B) {
                // Mark facets we need as as needed.
                #(
                    <B as ::#facet_crate::AsyncBuilderFor<
                        ::std::sync::Arc<#facet_types>
                    >>::need(builder);
                )*

                // Mark facets our delegates need as needed.
                #(
                    <#delegate_types as ::#facet_crate::AsyncBuildable<'builder, B>>
                        ::mark_needed(builder);
                )*
           }

            fn construct(builder: &B) -> Self {
                // Build delegates.
                #(
                    let #delegate_idents =
                        <#delegate_types as ::#facet_crate::AsyncBuildable<'builder, B>>
                            ::construct(builder);
                )*

                // Get the facets out of the builder.
                #(
                    let #facet_idents =
                        <B as ::#facet_crate::AsyncBuilderFor<
                            ::std::sync::Arc<#facet_types>
                        >>::get(builder);
                )*

                // Initialize other fields.
                #(
                    let #field_idents = #field_inits;
                )*

                Self {
                    #( #delegate_idents, )*
                    #( #field_idents, )*
                    #( #facet_idents, )*
                }
            }
        }

    }
}

fn gen_attr_impls(
    facet_crate: &Ident,
    container_name: &Ident,
    members: &ContainerMembers,
) -> Vec<TokenStream> {
    let mut output = Vec::new();
    let facet_idents = &members.facet_idents;
    let facet_types = &members.facet_types;
    let delegate_idents = &members.delegate_idents;
    let delegate_facets = &members.delegate_facets;

    for (facet_ident, facet_type) in facet_idents.iter().zip(facet_types) {
        output.push(quote! {
            impl ::#facet_crate::FacetRef<#facet_type> for #container_name {
                #[inline]
                fn facet_ref(&self) -> &(#facet_type)
                {
                    self.#facet_ident.as_ref()
                }
            }

            impl ::#facet_crate::FacetRef<#facet_type> for &#container_name {
                #[inline]
                fn facet_ref(&self) -> &(#facet_type)
                {
                    (*self).#facet_ident.as_ref()
                }
            }

            impl ::#facet_crate::FacetArc<#facet_type> for #container_name {
                #[inline]
                fn facet_arc(&self) -> ::std::sync::Arc<#facet_type>
                {
                    self.#facet_ident.clone()
                }
            }

            impl ::#facet_crate::FacetArc<#facet_type> for &#container_name {
                #[inline]
                fn facet_arc(&self) -> ::std::sync::Arc<#facet_type>
                {
                    (*self).#facet_ident.clone()
                }
            }

        });
    }

    for (delegate_ident, delegate_facet) in delegate_idents.iter().zip(delegate_facets) {
        output.push(quote! {
            #(
                impl ::#facet_crate::FacetRef<#delegate_facet> for #container_name {
                    #[inline]
                    fn facet_ref(&self) -> &(#delegate_facet) {
                        self.#delegate_ident.facet_ref()
                    }
                }

                impl ::#facet_crate::FacetRef<#delegate_facet> for &#container_name {
                    #[inline]
                    fn facet_ref(&self) -> &(#delegate_facet) {
                        self.#delegate_ident.facet_ref()
                    }
                }

                impl ::#facet_crate::FacetArc<#delegate_facet> for #container_name {
                    #[inline]
                    fn facet_arc(&self) -> ::std::sync::Arc<#delegate_facet> {
                        self.#delegate_ident.facet_arc()
                    }
                }

                impl ::#facet_crate::FacetArc<#delegate_facet> for &#container_name {
                    #[inline]
                    fn facet_arc(&self) -> ::std::sync::Arc<#delegate_facet> {
                        self.#delegate_ident.facet_arc()
                    }
                }
            )*
        });
    }

    output
}

fn extract_delegate_facets(attr: &Attribute) -> Result<Vec<Type>, Error> {
    let mut facets = Vec::new();
    let args: Punctuated<Type, Token![,]> = attr.parse_args_with(Punctuated::parse_terminated)?;
    for mut arg in args {
        if let Type::TraitObject(obj) = &mut arg {
            obj.bounds.push(syn::parse2(quote!(::std::marker::Send))?);
            obj.bounds.push(syn::parse2(quote!(::std::marker::Sync))?);
            obj.bounds.push(syn::parse2(quote!('static))?);
        }
        facets.push(arg);
    }
    Ok(facets)
}
