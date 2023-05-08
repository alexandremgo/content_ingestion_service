use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream, Result},
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
    fn parse(input: ParseStream) -> Result<Self> {
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
    // Parse the attribute arguments.
    let attr_args = parse_macro_input!(attr as Args);

    if attr_args.vars.len() < 1 {
        panic!("Describe the test in at least one sentence");
    }

    let description: Vec<String> = attr_args
        .vars
        .iter()
        .map(|literal| {
            if let Lit::Str(lit_str) = literal {
                lit_str.value()
            } else {
                panic!("The args should only be string");
            }
        })
        .collect();

    let description = description.join("\n");
    println!("Ok {description}");

    let input = parse_macro_input!(item as ItemFn);

    // Extracts the name and block of the input function.
    let fn_name = input.sig.ident;
    println!("Signature: 🦖 {:?}", fn_name);
    let fn_block = input.block;

    quote! {
        fn #fn_name() {
            println!(#description);
            #fn_block
        }
    }
    .into()
}

// #[cfg(test)]
// mod test {
//     use super::*;

//     #[test]
//     #[t_describe("ok")]
//     fn it_should_print_description() {

//     }
// }
