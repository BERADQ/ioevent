//! Procedural macros for the I/O event system.
//!
//! This crate provides procedural macros for deriving event types and creating event subscribers.
//! It is part of the `ioevent` ecosystem and should be used as a dependency of `ioevent`.

use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{FnArg, ItemFn, ReturnType, Token, parse_macro_input, punctuated::Punctuated};

/// Derives the `Event` trait for a type.
///
/// This macro implements the `Event` trait for a type, allowing it to participate in the event system.
/// It provides serialization and deserialization capabilities for the type.
///
/// # Attributes
///
/// * `#[event(tag = "custom_tag")]` - Specifies a custom tag for the event type.
///   If not provided, the tag will be generated from the module path and type name.
///   
/// # Requires
///
/// * The type must implement the `Serialize` and `Deserialize` traits from the `serde` crate.
///
/// # Examples
///
/// ```rust
/// #[derive(Event)]
/// struct MyEvent {
///     field: String,
/// }
/// ```
#[proc_macro_derive(Event, attributes(event))]
pub fn derive_event(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    let name = input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let mut custom_tag = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("event") {
            continue;
        }

        let meta_list =
            match attr.parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated) {
                Ok(list) => list,
                Err(e) => return e.to_compile_error().into(),
            };

        for meta in meta_list {
            match meta {
                syn::Meta::NameValue(nv) if nv.path.is_ident("tag") => {
                    let lit_str =
                        match syn::parse2::<syn::LitStr>(nv.value.clone().into_token_stream()) {
                            Ok(lit) => lit,
                            Err(_) => {
                                let msg = "`tag` attribute must be a string literal";
                                return syn::Error::new_spanned(nv.value, msg)
                                    .to_compile_error()
                                    .into();
                            }
                        };

                    if custom_tag.is_some() {
                        let msg = "`tag` specified multiple times";
                        return syn::Error::new_spanned(nv, msg).to_compile_error().into();
                    }

                    custom_tag = Some(lit_str);
                }
                _ => {
                    let msg = "unknown attribute parameter, expected `tag = \"...\"`";
                    return syn::Error::new_spanned(meta, msg).to_compile_error().into();
                }
            }
        }
    }

    let tag_expr = if let Some(lit) = custom_tag {
        quote! { #lit }
    } else {
        quote! { concat!(module_path!(), "::", stringify!(#name)) }
    };

    let expanded = quote! {
        impl #impl_generics ::ioevent::event::Event for #name #ty_generics #where_clause {
            const TAG: &'static str = #tag_expr;
        }

        impl #impl_generics TryFrom<&::ioevent::event::EventData> for #name #ty_generics #where_clause {
            type Error = ::ioevent::error::TryFromEventError;
            fn try_from(value: &::ioevent::event::EventData) -> ::core::result::Result<Self, Self::Error> {
                ::core::result::Result::Ok(value.payload.deserialized()?)
            }
        }
    };

    TokenStream::from(expanded)
}

/// Creates an event subscriber from an async function.
///
/// This macro transforms an async function into an event subscriber that can be registered
/// with the event system. The function must take either one or two parameters:
/// * A state parameter (optional)
/// * An event parameter
/// * A return value of type `Result` (optional)
///
/// # Examples
///
/// ```rust
/// #[subscriber]
/// async fn handle_event(event: MyEvent) -> Result {
///     // Handle the event
///     Ok(())
/// }
/// ```
#[proc_macro_attribute]
pub fn subscriber(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let original_fn = parse_macro_input!(item as ItemFn);

    if original_fn.sig.asyncness.is_none() {
        return quote! { compile_error!("subscriber macro can only be applied to async functions"); }.into();
    }

    let params = original_fn.sig.inputs.iter().collect::<Vec<_>>();
    let (state_param, event_param) = match params.len() {
        1 => (None, params[0]),
        2 => (Some(params[0]), params[1]),
        _ => panic!("Expected 1 or 2 parameters"),
    };

    let (event_ty, event_name) = match event_param {
        FnArg::Typed(pat_type) => (&pat_type.ty, &pat_type.pat),
        _ => panic!("Event parameter must be a typed parameter"),
    };

    let state_ty_name = state_param.map(|param| match param {
        FnArg::Typed(pat_type) => (&pat_type.ty, &pat_type.pat),
        _ => panic!("State parameter must be a typed parameter"),
    });
    
    let raw_generics = &original_fn.sig.generics.type_params().map(|v|v.clone()).collect::<Vec<_>>();

    let (generics, new_params) = if let Some((state_ty, state_name)) = state_ty_name {
        let params = quote! {
            #state_name: &#state_ty,
            #event_name: &::ioevent::event::EventData
        };
        (quote! { <#(#raw_generics),*> }, params)
    } else {
        let params = quote! {
            _state: &::ioevent::state::State<_STATE>,
            #event_name: &::ioevent::event::EventData
        };
        (quote! { <#(#raw_generics),* _STATE> }, params)
    };

    let event_try_into = quote! {
        let #event_name: ::core::result::Result<#event_ty, ::ioevent::error::TryFromEventError> = ::std::convert::TryInto::try_into(#event_name);
    };

    let state_clone = if let Some((_, state_name)) = state_ty_name {
        quote! {
            let #state_name = ::std::clone::Clone::clone(#state_name);
        }
    } else {
        quote! {}
    };

    let return_expr = if matches!(original_fn.sig.output, ReturnType::Default) {
        Some(quote! { Ok(()) })
    } else {
        None
    };

    let original_stmts = &original_fn.block.stmts;

    let async_block = quote! {
        async move {
            let #event_name = #event_name?;
            #(#original_stmts)*
            #return_expr
        }
    };

    let func_name = &original_fn.sig.ident;

    let mod_name = format_ident!("{}", func_name);

    let vis = &original_fn.vis;

    let mod_block = quote! {
        #[doc(hidden)]
        #vis mod #mod_name {
            use super::*;
            pub type _Event = #event_ty;
        }
    };

    let expanded = quote! {
        #vis fn #func_name #generics (#new_params) -> ::ioevent::future::SubscribeFutureRet {
            #event_try_into
            #state_clone
            ::std::boxed::Box::pin(#async_block)
        }
        #mod_block
    };

    TokenStream::from(expanded)
}

/// Derives the `ProcedureCall` trait for a type.
///
/// This macro implements the `ProcedureCall` trait for a type, allowing it to be used
/// in remote procedure calls. It provides serialization and deserialization capabilities
/// for the type.
///
/// # Attributes
///
/// * `#[procedure(path = "custom_path")]` - Specifies a custom path for the procedure.
///   If not provided, the path will be generated from the module path and type name.
///   
/// # Requires
///
/// * The type must implement the `Serialize` and `Deserialize` traits from the `serde` crate.
///
/// # Examples
///
/// ```rust
/// #[derive(ProcedureCall)]
/// struct MyProcedure {
///     field: String,
/// }
/// ```
#[proc_macro_derive(ProcedureCall, attributes(procedure))]
pub fn derive_procedure_call(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    let name = input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let mut custom_path = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("procedure") {
            continue;
        }

        let meta_list = match attr.parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated) {
            Ok(list) => list,
            Err(e) => return e.to_compile_error().into(),
        };

        for meta in meta_list {
            match meta {
                syn::Meta::NameValue(nv) if nv.path.is_ident("path") => {
                    let lit_str = match syn::parse2::<syn::LitStr>(nv.value.clone().into_token_stream()) {
                        Ok(lit) => lit,
                        Err(_) => {
                            let msg = "`path` attribute must be a string literal";
                            return syn::Error::new_spanned(nv.value, msg)
                                .to_compile_error()
                                .into();
                        }
                    };

                    if custom_path.is_some() {
                        let msg = "`path` specified multiple times";
                        return syn::Error::new_spanned(nv, msg).to_compile_error().into();
                    }

                    custom_path = Some(lit_str);
                }
                _ => {
                    let msg = "unknown attribute parameter, expected `path = \"...\"`";
                    return syn::Error::new_spanned(meta, msg).to_compile_error().into();
                }
            }
        }
    }

    let path_expr = if let Some(lit) = custom_path {
        quote! { #lit }
    } else {
        quote! { concat!(module_path!(), "::", stringify!(#name)) }
    };

    let expanded = quote! {
        impl #impl_generics ::ioevent::state::ProcedureCall for #name #ty_generics #where_clause {
            fn path() -> String {
                #path_expr.to_owned()
            }
        }

        impl #impl_generics TryFrom<::ioevent::state::ProcedureCallData> for #name #ty_generics #where_clause {
            type Error = ::ioevent::error::TryFromEventError;
            fn try_from(value: ::ioevent::state::ProcedureCallData) -> ::core::result::Result<Self, Self::Error> {
                ::core::result::Result::Ok(value.payload.deserialized()?)
            }
        }
    };

    TokenStream::from(expanded)
}

/// Creates a procedure handler from an async function.
///
/// This macro transforms an async function into a procedure handler that can be registered
/// with the procedure call system. The function must take either one or two parameters:
/// * A state parameter (optional)
/// * A procedure parameter
///
/// # Examples
///
/// ```rust
/// #[procedure]
/// async fn handle_procedure(proc: MyProcedureRequest) -> Result {
///     // Handle the procedure
///     Ok(MyProcedureResponse)
/// }
/// ```
#[proc_macro_attribute]
pub fn procedure(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let original_fn = parse_macro_input!(item as ItemFn);

    if original_fn.sig.asyncness.is_none() {
        return quote! { compile_error!("procedure macro can only be applied to async functions"); }.into();
    }
    
    let params = original_fn.sig.inputs.iter().collect::<Vec<_>>();
    let (state_param, event_param) = match params.len() {
        1 => (None, params[0]),
        2 => (Some(params[0]), params[1]),
        _ => panic!("Expected 1 or 2 parameters"),
    };

    let (event_ty, event_name) = match event_param {
        FnArg::Typed(pat_type) => (&pat_type.ty, &pat_type.pat),
        _ => panic!("Event parameter must be a typed parameter"),
    };

    let state_ty_name = state_param.map(|param| match param {
        FnArg::Typed(pat_type) => (&pat_type.ty, &pat_type.pat),
        _ => panic!("State parameter must be a typed parameter"),
    });
    
    let raw_generics = &original_fn.sig.generics.type_params().map(|v|v.clone()).collect::<Vec<_>>();

    let (generics, new_params) = if let Some((state_ty, state_name)) = state_ty_name {
        let params = quote! {
            #state_name: &#state_ty,
            #event_name: &::ioevent::event::EventData
        };
        (quote! { <#(#raw_generics),*> }, params)
    } else {
        let params = quote! {
            _state: &::ioevent::state::State<_STATE>,
            #event_name: &::ioevent::event::EventData
        };
        (quote! { <#(#raw_generics),* _STATE: ::ioevent::state::ProcedureCallWright + ::std::clone::Clone + ::std::marker::Send + ::std::marker::Sync + 'static> }, params)
    };

    let event_try_into = quote! {
        let #event_name: ::core::result::Result<::ioevent::state::ProcedureCallData, ::ioevent::error::TryFromEventError> = ::std::convert::TryInto::try_into(#event_name);
    };

    let state_clone = if let Some((_, state_name)) = state_ty_name {
        quote! {
            let #state_name = ::std::clone::Clone::clone(#state_name);
        }
    } else {
        quote! {
            let _state = ::std::clone::Clone::clone(_state);
        }
    };

    let original_stmts = &original_fn.block.stmts;

    let async_block = if let Some((_, state_name)) = state_ty_name {
        quote! {
            async move {
                let #event_name = #event_name?;
                if <#event_ty as ::ioevent::state::ProcedureCallRequest>::match_self(&#event_name) {
                    let echo = #event_name.echo;
                    let #event_name = <#event_ty as ::std::convert::TryFrom<::ioevent::state::ProcedureCallData>>::try_from(#event_name)?;
                    let response: ::core::result::Result<_, ::ioevent::error::CallSubscribeError> = {
                        #(#original_stmts)*
                    };
                    ::ioevent::state::ProcedureCallExt::resolve::<#event_ty>(&#state_name, echo, &response?).await?;
                }
                Ok(())
            }
        }
    } else {
        quote! {
            async move {
                let #event_name = #event_name?;
                if <#event_ty as ::ioevent::state::ProcedureCallRequest>::match_self(&#event_name) {
                    let echo = #event_name.echo;
                    let #event_name = <#event_ty as ::std::convert::TryFrom<::ioevent::state::ProcedureCallData>>::try_from(#event_name)?;
                    let response: ::core::result::Result<_, ::ioevent::error::CallSubscribeError> = {
                        #(#original_stmts)*
                    };
                    ::ioevent::state::ProcedureCallExt::resolve::<#event_ty>(&_state, echo, &response?).await?;
                }
                Ok(())
            }
        }
    };

    let func_name = &original_fn.sig.ident;
    let mod_name = format_ident!("{}", func_name);

    let vis = &original_fn.vis;

    let mod_block = quote! {
        #[doc(hidden)]
        #vis mod #mod_name {
            use super::*;
            pub type _Event = ::ioevent::state::ProcedureCallData;
        }
    };

    let expanded = quote! {
        #vis fn #func_name #generics (#new_params) -> ::ioevent::future::SubscribeFutureRet {
            #event_try_into
            #state_clone
            ::std::boxed::Box::pin(#async_block)
        }
        #mod_block
    };

    TokenStream::from(expanded)
}