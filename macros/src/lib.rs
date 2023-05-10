use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    ItemFn, Lit, Token,
};

/// Represents our list of args, separated by commas. Each args is a literal
/// #[macro("literal 1", 2, "literal 3")]
struct Args {
    vars: Vec<Lit>,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        //
        let vars = Punctuated::<Lit, Token![,]>::parse_terminated(input)?;
        Ok(Args {
            vars: vars.into_iter().collect(),
        })
    }
}

// TODO: setup log env
// TODO: handles if there are other macros:
// For ex:
//
// #[t_describe(
//     "On a more complex and correct EPUB content",
//     "it should extract the content correctly in 1 yield"
// )]
// #[test]
// fn test() { ... }
#[proc_macro_attribute]
pub fn t_describe(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parses the attribute arguments.
    let attr_args = parse_macro_input!(attr as Args);

    // Parses the elements on which the macro is applied to
    let input = parse_macro_input!(item as ItemFn);

    t_describe_implementation(&attr_args, &input)
        .unwrap()
        .into()
}

fn t_describe_implementation(attr_args: &Args, item_fn: &ItemFn) -> Result<TokenStream2, String> {
    if attr_args.vars.len() < 1 {
        panic!("Describe the test in at least one sentence");
    }

    let description = attr_args
        .vars
        .iter()
        .map(|literal| {
            if let Lit::Str(lit_str) = literal {
                Ok(lit_str.value())
            } else {
                return Err("The args should only be string".to_string());
            }
        })
        .collect::<Result<Vec<String>, String>>()?;

    let description = description.join("\n");
    println!("Ok {description}");

    // Extracts the name and block of the input function.
    let fn_name = &item_fn.sig.ident;
    println!("Signature: 🦖 {:?}", fn_name);
    let fn_block = &item_fn.block;

    let result = quote! {
        fn #fn_name() {
            println!(#description);
            #fn_block
        }
    };

    Ok(result)
}

#[cfg(test)]
mod test_macro {
    use super::*;
    use syn::{parse2, ItemFn};

    #[test]
    fn it_should_print_description() {
        let input_token_stream = quote! {
            fn tested_fn(param1: usize) -> Result<(), ()> {
                let result = doing_something(param1);
                Ok(())
            }
        };

        let expected_token_stream = quote! {
            fn tested_fn(param1: usize) -> Result<(), ()> {
                println!("Test text");
                let result = doing_something(param1);
                Ok(())
            }
        };

        let input = parse2::<ItemFn>(input_token_stream).unwrap();

        let attr = quote! {
            "Test text"
        };

        let attr_args = parse2::<Args>(attr).unwrap();

        let result = t_describe_implementation(&attr_args, &input).unwrap();

        println!("Result: {}", result.to_string());

        assert_eq!(result.to_string(), expected_token_stream.to_string());
    }
}
