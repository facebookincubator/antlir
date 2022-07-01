use convert_case::Case;
use convert_case::Casing;
use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;
use syn::spanned::Spanned;
use syn::Data;
use syn::DeriveInput;
use syn::Error;
use syn::Fields;
use syn::Type;

fn expand_systemd_enum(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = input.ident.clone();
    match input.data {
        Data::Enum(e) => {
            let to_strs = e.variants.iter().map(|v| {
                let i = v.ident.clone();
                let name = i.to_string().to_case(Case::Kebab);
                match name.as_str() {
                    "unknown" => match v.fields {
                        Fields::Unit => quote! {
                            Self::Unknown => "unknown"
                        },
                        _ => quote! {
                            Self::Unknown(v) => v,
                        },
                    },
                    _ => quote! {
                        Self::#i => #name,
                    },
                }
            });
            let from_strs = e.variants.iter().map(|v| {
                let i = v.ident.clone();
                let name = i.to_string().to_case(Case::Kebab);
                match name.as_str() {
                    "unknown" => match v.fields {
                        Fields::Unit => quote! {
                            _ => Self::Unknown,
                        },
                        _ => quote! {
                            value => Ok(Self::Unknown(value.to_owned())),
                        },
                    },
                    _ => quote! {
                        #name => Ok(Self::#i),
                    },
                }
            });
            Ok(quote! {
                impl ::zvariant::Type for #name {
                    fn signature() -> ::zvariant::Signature<'static> {
                        <String as ::zvariant::Type>::signature()
                    }
                }

                impl ::std::str::FromStr for #name {
                    type Err = ::zvariant::Error;
                    fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                        match s {
                            #(#from_strs)*
                            _ => Err(::zvariant::Error::Message(format!("Unknown variant {}", s))),
                        }
                    }
                }

                impl ::std::fmt::Display for #name {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        let s = match self {
                            #(#to_strs)*
                        };
                        write!(f, "{}", s)
                    }
                }

                impl<'de> ::serde::Deserialize<'de> for #name {
                    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
                    where D: ::serde::Deserializer<'de> {
                        let s: String = ::serde::Deserialize::deserialize(deserializer)?;
                        s.parse().map_err(::serde::de::Error::custom)
                    }
                }

                impl ::serde::Serialize for #name {
                    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
                    where S: ::serde::Serializer {
                        serializer.serialize_str(&self.to_string())
                    }
                }

                impl<'v> ::std::convert::TryFrom<::zvariant::Value<'v>> for #name {
                    type Error = ::zvariant::Error;

                    fn try_from(v: ::zvariant::Value<'v>) -> ::zvariant::Result<Self> {
                        String::try_from(v).and_then(|v| v.parse())
                    }
                }

                impl ::std::convert::TryFrom<::zvariant::OwnedValue> for #name {
                    type Error = ::zvariant::Error;

                    fn try_from(v: ::zvariant::OwnedValue) -> ::zvariant::Result<Self> {
                        String::try_from(v).and_then(|v| v.parse())
                    }
                }

                impl ::std::convert::From<#name> for ::zvariant::OwnedValue {
                    fn from(v: #name) -> Self {
                        ::zvariant::Str::from(v.to_string()).into()
                    }
                }
            })
        }
        _ => Err(Error::new(input.span(), "not enum")),
    }
}

#[proc_macro_derive(SystemdEnum)]
pub fn derive_systemd_enum(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_systemd_enum(input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

fn expand_transparent_zvariant(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = input.ident.clone();
    let input_span = input.span();
    match input.data {
        Data::Struct(s) => match s.fields {
            Fields::Unnamed(fields) => match fields.unnamed.iter().collect::<Vec<_>>().as_slice() {
                [f] => {
                    let ty = &f.ty;
                    // I can't figure out / believe that it's impossible to
                    // implement a generic From<T> for #name where T: Into<#ty>,
                    // so for practical usage I'm going to special case where ty
                    // is String so that we have a From<&str> impl
                    let s: Type = syn::parse_str("String").unwrap();
                    let fqs: Type = syn::parse_str("std::string::String").unwrap();
                    let mut maybe_str_special = quote! {};
                    if ty == &s || ty == &fqs {
                        maybe_str_special = quote! {
                            impl ::std::convert::From<&str> for #name {
                                #[inline]
                                fn from(s: &str) -> Self {
                                    Self(s.to_owned())
                                }
                            }

                            impl ::std::cmp::PartialEq<&str> for #name {
                                #[inline]
                                fn eq(&self, s: &&str) -> bool {
                                    self.0 == *s
                                }
                            }

                            impl ::std::cmp::PartialEq<#name> for &str {
                                #[inline]
                                fn eq(&self, o: &#name) -> bool {
                                    *self == o.0
                                }
                            }

                            impl ::std::convert::AsRef<str> for #name {
                                fn as_ref(&self) -> &str {
                                    self.0.as_str()
                                }
                            }
                        };
                    }

                    Ok(quote! {
                        impl ::zvariant::Type for #name {
                            fn signature() -> ::zvariant::Signature<'static> {
                                <#ty as ::zvariant::Type>::signature()
                            }
                        }

                        impl<'v> ::std::convert::TryFrom<::zvariant::Value<'v>> for #name {
                            type Error = ::zvariant::Error;

                            fn try_from(v: ::zvariant::Value<'v>) -> ::zvariant::Result<Self> {
                                #ty::try_from(v).map(Self)
                            }
                        }

                        impl ::std::convert::TryFrom<::zvariant::OwnedValue> for #name {
                            type Error = ::zvariant::Error;

                            fn try_from(v: ::zvariant::OwnedValue) -> ::zvariant::Result<Self> {
                                #ty::try_from(v).map(Self)
                            }
                        }

                        impl<'de> ::serde::Deserialize<'de> for #name {
                            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
                            where D: ::serde::Deserializer<'de> {
                                ::serde::Deserialize::deserialize(deserializer).map(Self)
                            }
                        }

                        impl ::serde::Serialize for #name {
                            fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
                            where S: ::serde::Serializer {
                                self.0.serialize(serializer)
                            }
                        }

                        impl ::std::convert::From<#ty> for #name {
                            fn from(x: #ty) -> Self {
                                Self(x)
                            }
                        }

                        impl ::std::convert::From<#name> for #ty {
                            fn from(x: #name) -> #ty {
                                x.0
                            }
                        }

                        impl ::std::fmt::Display for #name {
                            fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                                self.0.fmt(f)
                            }
                        }

                        impl ::std::cmp::PartialEq<#ty> for #name {
                            fn eq(&self, o: &#ty) -> bool {
                                self.0 == *o
                            }
                        }

                        impl ::std::cmp::PartialEq<#name> for #ty {
                            fn eq(&self, o: &#name) -> bool {
                                *self == o.0
                            }
                        }

                        impl ::std::convert::AsRef<#ty> for #name {
                            fn as_ref(&self) -> &#ty {
                                &self.0
                            }
                        }

                        #maybe_str_special
                    })
                }
                _ => Err(Error::new(input_span, "struct must have one unnamed field")),
            },
            _ => Err(Error::new(input_span, "struct must have one unnamed field")),
        },
        _ => Err(Error::new(input_span, "not struct")),
    }
}

#[proc_macro_derive(TransparentZvariant)]
pub fn derive_transparent_zvariant(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_transparent_zvariant(input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}
