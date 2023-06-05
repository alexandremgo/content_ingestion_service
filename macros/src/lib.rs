use std::sync::Mutex;

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

/// Helper procedural macro to specifies a description for a test and setup log
///
/// Prints the test description specifies in the macro.
/// Setup pretty_env_logger if not yet initialized.
///
/// TODO: dependency on `memory_logger`
///
/// Ex usage:
/// ```
/// #[t_describe(
///     "On a more complex and correct EPUB content",
///     "it should extract the content correctly in 1 yield"
/// )]
/// #[test]
/// fn test() { ... }
/// ```
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

/// `t_describe` implementation using types defined in `proc_macro2`
///
/// Types from `proc_macro` are entirely specific to procedural macros and
/// cannot ever exist in code outside of a procedural macro.
/// Thus they cannot be unit-tested directly.
///
/// Meanwhile with types like `TokenStream2` from `proc_macro2`, the function
/// can be unit-tested below.
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

    let description = description.join("\n-> ");

    let fn_other_macros = &item_fn.attrs;

    // Extracts the name, return type and block of the input function from the function signature.
    let fn_name = &item_fn.sig.ident;
    let mut fn_block = item_fn.block.to_owned();
    let fn_return_type = &item_fn.sig.output;
    let fn_args = &item_fn.sig.inputs;

    // fn_block.stmts.insert(
    //     0,
    //     syn::parse2(quote! {
    //         println!("Test: {}", #description);
    //     })
    //     .unwrap(),
    // );

    // fn_block.stmts.insert(
    //     0,
    //     syn::parse2(quote! {
    //         pretty_env_logger::try_init();
    //     })
    //     .unwrap(),
    // );

    // TODO: HERE
    // Is it possible to capture the log ? And print them only on failure ? And not just randomly setting pretty_env_logger ?
    // let my_vec = MY_STATE.lock().unwrap();
    // println!("{:?}", my_vec);

    // A test function returns a `Result` and the Rust analyzer warns about "unused `Result` that must be used"
    // on the new generated tested function. `#[allow(unused)]` is needed to avoid this warning.
    // `fn_other_macros` is a Vec that needs to be iterated over using the repetition `#(...)*`
    let result = quote! {
            #[allow(unused)]
            #(#fn_other_macros)*
            fn #fn_name(#fn_args) #fn_return_type {
                // Shared stated which is dropped after all instances using the state go out of scope.
                lazy_static::lazy_static! {
                    static ref MY_STATE: Mutex<Vec<String>> = Mutex::new(Vec::new());
                }

                MY_STATE.lock().unwrap().push(format!("🔥 test: {:?}", "ok"));

                fn fn_implementation() #fn_return_type
                    #fn_block

                match std::panic::catch_unwind(|| fn_implementation()) {
                    Ok(_) => println!("\n----------\n✅ Success {}\n----------\n{:?}\n\n", #description, MY_STATE.lock().unwrap()),
                    Err(err) => panic!("\n----------\n🚨 Failure {}\n{}\n----------\n{:?}\n\n", #description, err.downcast::<String>().unwrap(), MY_STATE.lock().unwrap()),
                }
            }
        };

    Ok(result)
}

#[cfg(test)]
mod test_macro {
    use super::*;
    use syn::{parse2, ItemFn};

    #[test]
    fn on_simple_function_it_should_add_print_description_and_log_setup() {
        let input_token_stream = quote! {
            fn tested_fn(param1: usize) -> Result<(), ()> {
                let result = doing_something(param1);
                Ok(())
            }
        };

        let expected_token_stream = quote! {
            #[allow(unused)]
            fn tested_fn(param1: usize) -> Result<(), ()> {
                pretty_env_logger::try_init();
                println!("Test: {}", "Test text");
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

    #[test]
    fn on_function_with_several_description_it_should_add_print_description_and_log_setup() {
        let input_token_stream = quote! {
            fn tested_fn(param1: usize) -> Result<(), ()> {
                let result = doing_something(param1);
                Ok(())
            }
        };

        let expected_token_stream = quote! {
            #[allow(unused)]
            fn tested_fn(param1: usize) -> Result<(), ()> {
                pretty_env_logger::try_init();
                println!("Test: {}", "Description 1\n-> Description 2");
                let result = doing_something(param1);
                Ok(())
            }
        };

        let input = parse2::<ItemFn>(input_token_stream).unwrap();

        let attr = quote! {
            "Description 1", "Description 2"
        };

        let attr_args = parse2::<Args>(attr).unwrap();

        let result = t_describe_implementation(&attr_args, &input).unwrap();

        println!("Result: {}", result.to_string());

        assert_eq!(result.to_string(), expected_token_stream.to_string());
    }

    #[test]
    fn on_function_with_a_macro_already_it_should_add_print_description_and_log_setup() {
        let input_token_stream = quote! {
            #[test]
            #[another_macro]
            fn tested_fn(param1: usize) -> Result<(), ()> {
                let result = doing_something(param1);
                Ok(())
            }
        };

        let expected_token_stream = quote! {
            #[allow(unused)]
            #[test]
            #[another_macro]
            fn tested_fn(param1: usize) -> Result<(), ()> {
                pretty_env_logger::try_init();
                println!("Test: {}", "Test text");
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
