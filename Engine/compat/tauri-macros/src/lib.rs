//! `#[tauri::command]` and `tauri::generate_handler!`, for an engine with no
//! Tauri in it.
//!
//! The engine's 390 commands were written for Tauri. Rather than touch every
//! one, these two macros keep the spelling and change what it means:
//! `#[command]` leaves the function exactly as written and adds a hidden
//! module of the same name whose `__invoke` takes the call's JSON arguments, fills in the
//! engine-provided ones (`AppHandle`, `State<T>`, a window), runs the command
//! — on the blocking pool if it is an ordinary function, as a future if it is
//! async — and hands back JSON. `generate_handler!` turns the list the app
//! already keeps into a name → sibling table.
//!
//! Arguments are looked up the way Tauri looked them up — by the camelCase
//! form of the parameter's name — so the tool pages' `invoke()` calls keep
//! working unchanged. The snake_case name is accepted too.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, FnArg, ItemFn, Pat, Path, ReturnType, Token, Type};

#[proc_macro_attribute]
pub fn command(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    match wrapper(&function) {
        Ok(extra) => quote!(#function #extra).into(),
        Err(error) => {
            let error = error.to_compile_error();
            quote!(#function #error).into()
        }
    }
}

/// What a parameter is filled with.
enum Source {
    App,
    State(Type),
    Window,
    Json(Type),
}

fn last_segment(ty: &Type) -> Option<&syn::PathSegment> {
    match ty {
        Type::Path(path) => path.path.segments.last(),
        Type::Reference(reference) => last_segment(&reference.elem),
        _ => None,
    }
}

fn source_of(ty: &Type) -> Source {
    let Some(segment) = last_segment(ty) else {
        return Source::Json(ty.clone());
    };
    match segment.ident.to_string().as_str() {
        "AppHandle" => Source::App,
        "Window" | "WebviewWindow" | "Webview" => Source::Window,
        "State" => {
            if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                for arg in &args.args {
                    if let syn::GenericArgument::Type(inner) = arg {
                        return Source::State(inner.clone());
                    }
                }
            }
            Source::Json(ty.clone())
        }
        _ => Source::Json(ty.clone()),
    }
}

/// Tauri's rule: the parameter's name in lowerCamelCase, leading
/// underscores dropped.
fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for ch in name.trim_start_matches('_').chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn returns_result(output: &ReturnType) -> bool {
    match output {
        ReturnType::Default => false,
        ReturnType::Type(_, ty) => last_segment(ty).is_some_and(|s| s.ident == "Result"),
    }
}

fn wrapper(function: &ItemFn) -> syn::Result<TokenStream2> {
    let name = &function.sig.ident;
    let vis = &function.vis;
    let is_async = function.sig.asyncness.is_some();

    let mut bindings = Vec::new();
    let mut call_args = Vec::new();
    for (index, input) in function.sig.inputs.iter().enumerate() {
        let FnArg::Typed(typed) = input else {
            return Err(syn::Error::new_spanned(input, "commands can't take self"));
        };
        let raw = match &*typed.pat {
            Pat::Ident(ident) => ident.ident.to_string(),
            _ => format!("arg{index}"),
        };
        let local = format_ident!("__arg{}", index);
        let ty = &typed.ty;
        let binding = match source_of(ty) {
            Source::App => quote!(let #local = __invoke.app.clone();),
            Source::State(inner) => quote!(let #local = ::tauri::ipc::state::<#inner>(&__invoke.app);),
            Source::Window => quote!(let #local = <#ty as ::tauri::ipc::FromInvoke>::from_invoke(&__invoke);),
            Source::Json(ty) => {
                let camel_key = camel(&raw);
                let snake_key = raw.trim_start_matches('_').to_string();
                quote! {
                    let #local: #ty = match ::tauri::ipc::arg(&__invoke.args, #camel_key, #snake_key) {
                        Ok(value) => value,
                        Err(error) => return Box::pin(async move { Err(error) }),
                    };
                }
            }
        };
        bindings.push(binding);
        call_args.push(local);
    }

    let convert = if returns_result(&function.sig.output) {
        quote!(::tauri::ipc::from_result(__out))
    } else {
        quote!(::tauri::ipc::from_value(__out))
    };

    let run = if is_async {
        quote! {
            Box::pin(async move {
                let __out = super::#name(#(#call_args),*).await;
                #convert
            })
        }
    } else {
        quote! {
            Box::pin(async move {
                match ::tauri::async_runtime::spawn_blocking(move || super::#name(#(#call_args),*)).await {
                    Ok(__out) => #convert,
                    Err(error) => Err(::tauri::ipc::panicked(error)),
                }
            })
        }
    };

    // The entry point lives in a module named after the command. Modules and
    // functions are different namespaces, so the two share the name — and a
    // `use commands::x::name;` brings both, the way `generate_handler!` needs.
    Ok(quote! {
        #[doc(hidden)]
        #[allow(non_snake_case, unused_imports, clippy::all)]
        #vis mod #name {
            use super::*;
            pub fn __invoke(__invoke: ::tauri::ipc::Invoke) -> ::tauri::ipc::CommandFuture {
                #(#bindings)*
                #run
            }
        }
    })
}

struct HandlerList(Punctuated<Path, Token![,]>);

impl Parse for HandlerList {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(HandlerList(Punctuated::parse_terminated(input)?))
    }
}

/// `generate_handler![a::b, c]` → a `tauri::ipc::Handler`: every command by
/// name, and a way to find one.
#[proc_macro]
pub fn generate_handler(input: TokenStream) -> TokenStream {
    let HandlerList(paths) = parse_macro_input!(input as HandlerList);
    let mut names = Vec::new();
    let mut siblings = Vec::new();
    for path in paths {
        let name = path.segments.last().expect("a command path").ident.to_string();
        names.push(name);
        let mut entry = path.clone();
        entry.segments.push(syn::PathSegment::from(syn::Ident::new("__invoke", Span::call_site())));
        siblings.push(entry);
    }
    quote! {
        ::tauri::ipc::Handler {
            names: &[#(#names),*],
            find: |__name: &str| -> ::core::option::Option<::tauri::ipc::CommandFn> {
                match __name {
                    #(#names => ::core::option::Option::Some(#siblings as ::tauri::ipc::CommandFn),)*
                    _ => ::core::option::Option::None,
                }
            },
        }
    }
    .into()
}
