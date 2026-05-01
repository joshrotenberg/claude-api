//! Procedural macros for [`claude-api`](https://docs.rs/claude-api).
//!
//! Re-exported from the parent crate behind the `derive` feature; users
//! should write `claude_api::derive::Tool`, not depend on this crate
//! directly.
//!
//! # `#[derive(Tool)]`
//!
//! Derive an implementation of `claude_api::tool_dispatch::Tool` for a
//! struct that already implements `serde::Deserialize` and
//! `schemars::JsonSchema`. The struct's fields define the tool input;
//! the user supplies the behavior via an inherent `async fn run(self)`.
//!
//! ```ignore
//! use claude_api::derive::Tool;
//! use claude_api::tool_dispatch::ToolError;
//! use serde::Deserialize;
//! use schemars::JsonSchema;
//!
//! /// Get the current weather for a city.
//! #[derive(Deserialize, JsonSchema, Tool)]
//! struct GetWeather {
//!     /// City to look up.
//!     city: String,
//! }
//!
//! impl GetWeather {
//!     async fn run(self) -> Result<serde_json::Value, ToolError> {
//!         Ok(serde_json::json!({"temp": 72, "city": self.city}))
//!     }
//! }
//!
//! // Use:
//! let tool = GetWeather::tool();
//! ```
//!
//! ## Attribute syntax
//!
//! - `#[tool(name = "...")]` -- override the tool name.
//!   Default: `snake_case` of the type name.
//! - `#[tool(description = "...")]` -- override the description.
//!   Default: the first line of the struct's doc comment.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Lit, Meta, parse_macro_input};

/// Derive `claude_api::tool_dispatch::Tool` for a struct.
///
/// See the [crate-level docs](crate) for the supported attribute syntax and
/// the requirements on the underlying struct.
#[proc_macro_derive(Tool, attributes(tool))]
pub fn derive_tool(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_tool(&input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand_tool(input: &DeriveInput) -> syn::Result<TokenStream2> {
    if !matches!(input.data, Data::Struct(_)) {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "#[derive(Tool)] only supports structs",
        ));
    }

    let struct_ident = &input.ident;
    let attrs = ToolAttrs::parse(&input.attrs)?;

    let tool_name = attrs
        .name
        .unwrap_or_else(|| pascal_to_snake(&struct_ident.to_string()));
    let tool_description: Option<String> =
        attrs.description.or_else(|| first_doc_line(&input.attrs));

    let wrapper_ident = quote::format_ident!("__{}ToolImpl", struct_ident);

    let description_method: TokenStream2 = if let Some(desc) = &tool_description {
        quote! {
            fn description(&self) -> ::core::option::Option<&str> {
                ::core::option::Option::Some(#desc)
            }
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        // Hidden wrapper struct that carries the Tool impl. Unit struct so
        // it's trivially Send + Sync + 'static.
        #[doc(hidden)]
        #[derive(::core::default::Default)]
        pub struct #wrapper_ident;

        #[::claude_api::__private::async_trait::async_trait]
        impl ::claude_api::tool_dispatch::Tool for #wrapper_ident {
            fn name(&self) -> &str {
                #tool_name
            }

            #description_method

            fn schema(&self) -> ::claude_api::__private::serde_json::Value {
                let schema = ::claude_api::__private::schemars::schema_for!(#struct_ident);
                ::claude_api::__private::serde_json::to_value(&schema)
                    .unwrap_or_else(|_| ::claude_api::__private::serde_json::Value::Null)
            }

            async fn invoke(
                &self,
                input: ::claude_api::__private::serde_json::Value,
            ) -> ::core::result::Result<
                ::claude_api::__private::serde_json::Value,
                ::claude_api::tool_dispatch::ToolError,
            > {
                let parsed: #struct_ident =
                    ::claude_api::__private::serde_json::from_value(input)
                        .map_err(|e| ::claude_api::tool_dispatch::ToolError::invalid_input(
                            ::std::format!("input did not match {}'s schema: {}", #tool_name, e)
                        ))?;
                <#struct_ident>::run(parsed).await
            }
        }

        impl #struct_ident {
            /// Build the [`Tool`](::claude_api::tool_dispatch::Tool) impl
            /// derived for this type. The returned value is a unit struct
            /// implementing `Tool`; pass it to
            /// [`ToolRegistry::register_tool`](::claude_api::tool_dispatch::ToolRegistry::register_tool)
            /// or wrap in `Arc` for trait-object dispatch.
            pub fn tool() -> #wrapper_ident {
                #wrapper_ident::default()
            }
        }
    })
}

#[derive(Default)]
struct ToolAttrs {
    name: Option<String>,
    description: Option<String>,
}

impl ToolAttrs {
    fn parse(attrs: &[syn::Attribute]) -> syn::Result<Self> {
        let mut out = ToolAttrs::default();
        for attr in attrs {
            if !attr.path().is_ident("tool") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    let value = meta.value()?;
                    let lit: syn::LitStr = value.parse()?;
                    out.name = Some(lit.value());
                } else if meta.path.is_ident("description") {
                    let value = meta.value()?;
                    let lit: syn::LitStr = value.parse()?;
                    out.description = Some(lit.value());
                } else {
                    return Err(meta
                        .error("unsupported #[tool(...)] key; expected `name` or `description`"));
                }
                Ok(())
            })?;
        }
        Ok(out)
    }
}

fn first_doc_line(attrs: &[syn::Attribute]) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: Lit::Str(s), ..
            }) = &nv.value
        {
            lines.push(s.value().trim().to_string());
        }
    }
    let joined = lines.join(" ");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        None
    } else {
        // First sentence: take up to and including the first period that
        // ends a sentence (followed by space, end-of-string, or end-of-line).
        let mut end = trimmed.len();
        for (i, ch) in trimmed.char_indices() {
            if ch == '.' {
                let after_idx = i + ch.len_utf8();
                let after = &trimmed[after_idx..];
                if after.is_empty() || after.starts_with(' ') {
                    end = after_idx;
                    break;
                }
            }
        }
        Some(trimmed[..end].to_string())
    }
}

fn pascal_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.char_indices() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_basic() {
        assert_eq!(pascal_to_snake("GetWeather"), "get_weather");
        assert_eq!(pascal_to_snake("HTMLParser"), "h_t_m_l_parser");
        assert_eq!(pascal_to_snake("F"), "f");
        assert_eq!(pascal_to_snake("Foo"), "foo");
    }

    #[test]
    fn first_doc_line_takes_first_sentence() {
        // Build attrs simulating: /// Hello world. More text here.
        let attrs: Vec<syn::Attribute> = syn::parse_quote! {
            /// Hello world. More text here.
        };
        assert_eq!(first_doc_line(&attrs).as_deref(), Some("Hello world."));
    }

    #[test]
    fn first_doc_line_handles_no_period() {
        let attrs: Vec<syn::Attribute> = syn::parse_quote! {
            /// Hello world
        };
        assert_eq!(first_doc_line(&attrs).as_deref(), Some("Hello world"));
    }

    #[test]
    fn first_doc_line_returns_none_on_empty() {
        let attrs: Vec<syn::Attribute> = vec![];
        assert!(first_doc_line(&attrs).is_none());
    }
}
