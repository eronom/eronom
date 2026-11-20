use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{LitByteStr, LitStr, Path, Token, parse_macro_input};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Values {
    #[default]
    None,
    One,
    Many,
    OneOptional,
}

#[derive(Default, Debug)]
struct Names {
    short: Option<u8>,
    long: Option<String>,
    long_aliases: Vec<String>,
}

#[derive(Default, Debug)]
struct Help {
    msg: String,
    value: String,
}

#[derive(Default, Debug)]
struct Param {
    id: Help,
    names: Names,
    takes_value: Values,
}

enum ParseParamError {
    NoParamFound,
    InvalidShortParam,
    TrailingComma,
}

impl ParseParamError {
    fn msg(&self) -> &'static str {
        match self {
            Self::NoParamFound => "no parameter found",
            Self::InvalidShortParam => "invalid short parameter (must be format `-x`)",
            Self::TrailingComma => "trailing comma after parameter name",
        }
    }
}

struct TokenizeAny<'a> {
    buf: &'a [u8],
    index: usize,
}

impl<'a> TokenizeAny<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, index: 0 }
    }
    fn is_delim(b: u8) -> bool {
        b == b' ' || b == b'\t'
    }
    fn next(&mut self) -> Option<&'a [u8]> {
        while self.index < self.buf.len() && Self::is_delim(self.buf[self.index]) {
            self.index += 1;
        }
        let start = self.index;
        if start == self.buf.len() {
            return None;
        }
        while self.index < self.buf.len() && !Self::is_delim(self.buf[self.index]) {
            self.index += 1;
        }
        Some(&self.buf[start..self.index])
    }
    fn rest(&mut self) -> &'a [u8] {
        while self.index < self.buf.len() && Self::is_delim(self.buf[self.index]) {
            self.index += 1;
        }
        &self.buf[self.index..]
    }
}

fn trim_left(s: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
        i += 1;
    }
    &s[i..]
}

fn trim(s: &[u8]) -> &[u8] {
    let s = trim_left(s);
    let mut e = s.len();
    while e > 0 && (s[e - 1] == b' ' || s[e - 1] == b'\t') {
        e -= 1;
    }
    &s[..e]
}

fn to_string(s: &[u8]) -> String {
    String::from_utf8(s.to_vec()).expect("param strings must be UTF-8")
}

fn parse_param(line: &[u8]) -> Result<Param, ParseParamError> {
    let mut found_comma = false;
    let mut it = TokenizeAny::new(line);
    let mut param_str = it.next().ok_or(ParseParamError::NoParamFound)?;

    let short_name: Option<u8> = if !param_str.starts_with(b"--") && param_str.starts_with(b"-") {
        found_comma = param_str[param_str.len() - 1] == b',';
        if found_comma {
            param_str = &param_str[..param_str.len() - 1];
        }
        if param_str.len() != 2 {
            return Err(ParseParamError::InvalidShortParam);
        }
        let short = param_str[1];
        if !found_comma {
            let mut res = parse_param_rest(it.rest());
            res.names.short = Some(short);
            return Ok(res);
        }
        param_str = it.next().ok_or(ParseParamError::NoParamFound)?;
        Some(short)
    } else {
        None
    };

    if param_str.starts_with(b"--") {
        if param_str[param_str.len() - 1] == b',' {
            return Err(ParseParamError::TrailingComma);
        }
    } else if found_comma {
        return Err(ParseParamError::TrailingComma);
    } else if short_name.is_none() {
        return Ok(parse_param_rest(trim_left(line)));
    }

    let mut res = parse_param_rest(it.rest());
    res.names.short = short_name;

    let long_names = parse_long_names(param_str);
    res.names.long = long_names.long;
    res.names.long_aliases = long_names.long_aliases;
    Ok(res)
}

fn parse_long_names(param_str: &[u8]) -> Names {
    let alias_count = param_str.iter().filter(|&&c| c == b'/').count();

    if alias_count == 0 {
        if param_str.starts_with(b"--") {
            return Names {
                long: Some(to_string(&param_str[2..])),
                ..Default::default()
            };
        }
        return Names::default();
    }

    let mut primary: Option<String> = None;
    let mut aliases: Vec<String> = Vec::with_capacity(alias_count);
    for name_part in param_str.split(|&b| b == b'/') {
        if !name_part.starts_with(b"--") {
            continue;
        }
        if primary.is_none() {
            primary = Some(to_string(&name_part[2..]));
        } else {
            aliases.push(to_string(&name_part[2..]));
        }
    }

    Names {
        long: primary,
        long_aliases: aliases,
        ..Default::default()
    }
}

fn parse_param_rest(line: &[u8]) -> Param {
    let trimmed = trim_left(line);
    if trimmed.starts_with(b"<") {
        if let Some(len) = trimmed.iter().position(|&b| b == b'>') {
            let after = &trimmed[len + 1..];
            let takes_many = after.starts_with(b"...");
            let takes_one_optional = after.starts_with(b"?");
            let help_start = len + 1 + 3 * (takes_many as usize) + (takes_one_optional as usize);
            return Param {
                takes_value: if takes_many {
                    Values::Many
                } else if takes_one_optional {
                    Values::OneOptional
                } else {
                    Values::One
                },
                id: Help {
                    msg: to_string(trim(&trimmed[help_start..])),
                    value: to_string(&trimmed[1..len]),
                },
                ..Default::default()
            };
        }
    }

    Param {
        id: Help {
            msg: to_string(trim(line)),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn byte_str(s: &str) -> LitByteStr {
    LitByteStr::new(s.as_bytes(), Span::call_site())
}

fn emit_param(krate: &Path, p: &Param) -> TokenStream2 {
    let msg = byte_str(&p.id.msg);
    let msg_plain = byte_str(&p.id.msg);
    let value = byte_str(&p.id.value);

    let short = match p.names.short {
        Some(c) => {
            let c = proc_macro2::Literal::u8_suffixed(c);
            quote! { ::core::option::Option::Some(#c) }
        }
        None => quote! { ::core::option::Option::None },
    };

    let long = match &p.names.long {
        Some(l) => {
            let l = byte_str(l);
            quote! { ::core::option::Option::Some(#l as &'static [u8]) }
        }
        None => quote! { ::core::option::Option::None },
    };

    let aliases = if p.names.long_aliases.is_empty() {
        quote! { &[] }
    } else {
        let items = p.names.long_aliases.iter().map(|a| {
            let a = byte_str(a);
            quote! { #a as &'static [u8] }
        });
        quote! { &[ #(#items),* ] }
    };

    let takes_value = match p.takes_value {
        Values::None => quote! { #krate::Values::None },
        Values::One => quote! { #krate::Values::One },
        Values::Many => quote! { #krate::Values::Many },
        Values::OneOptional => quote! { #krate::Values::OneOptional },
    };

    quote! {
        #krate::Param::<#krate::Help> {
            id: #krate::Help {
                msg: #msg,
                msg_plain: #msg_plain,
                value: #value,
            },
            names: #krate::Names {
                short: #short,
                long: #long,
                long_aliases: #aliases,
            },
            takes_value: #takes_value,
        }
    }
}

struct ParseParamInput {
    krate: Path,
    lit: LitStr,
}

impl Parse for ParseParamInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let krate: Path = input.parse()?;
        input.parse::<Token![,]>()?;
        let lit: LitStr = input.parse()?;
        let _ = input.parse::<Token![,]>();
        Ok(Self { krate, lit })
    }
}

#[proc_macro]
pub fn __parse_param_impl(input: TokenStream) -> TokenStream {
    let ParseParamInput { krate, lit } = parse_macro_input!(input as ParseParamInput);
    let line = lit.value();
    match parse_param(line.as_bytes()) {
        Ok(p) => emit_param(&krate, &p).into(),
        Err(e) => syn::Error::new(lit.span(), format!("parse_param!: {}", e.msg()))
            .to_compile_error()
            .into(),
    }
}

struct ParseParamsInput {
    krate: Path,
    lits: Punctuated<LitStr, Token![;]>,
}

impl Parse for ParseParamsInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let krate: Path = input.parse()?;
        input.parse::<Token![,]>()?;
        let lits = Punctuated::parse_terminated(input)?;
        Ok(Self { krate, lits })
    }
}

#[proc_macro]
pub fn __parse_params_impl(input: TokenStream) -> TokenStream {
    let ParseParamsInput { krate, lits } = parse_macro_input!(input as ParseParamsInput);
    let mut elements = Vec::new();
    for lit in lits {
        let line = lit.value();
        match parse_param(line.as_bytes()) {
            Ok(p) => elements.push(emit_param(&krate, &p)),
            Err(e) => {
                return syn::Error::new(lit.span(), format!("parse_params!: {}", e.msg()))
                    .to_compile_error()
                    .into();
            }
        }
    }
    let expanded = quote! {
        &[ #(#elements),* ]
    };
    expanded.into()
}
