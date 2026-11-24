use crate::args::ArgIter;
use crate::comptime::{ConvertedTable, Param, Values};
use crate::error::{Error, Result};

pub enum ArgItem<'p, 'a> {
    Option {
        param: &'p Param<usize>,
        value: Option<&'a [u8]>,
    },
    Positional(&'a [u8]),
}

enum State<'a> {
    Normal,
    Chaining { arg: &'a [u8], index: usize },
    RestArePositional,
}

pub struct StreamingClap<'p, 'a, I: ArgIter<'a>> {
    table: &'p ConvertedTable,
    iter: I,
    state: State<'a>,
}

impl<'p, 'a, I: ArgIter<'a>> StreamingClap<'p, 'a, I> {
    pub fn new(table: &'p ConvertedTable, iter: I) -> Self {
        Self {
            table,
            iter,
            state: State::Normal,
        }
    }

    pub fn next(&mut self) -> Result<Option<ArgItem<'p, 'a>>> {
        match self.state {
            State::Normal => self.next_normal(),
            State::Chaining { arg, index } => self.next_chaining(arg, index),
            State::RestArePositional => {
                if let Some(val) = self.iter.next() {
                    Ok(Some(ArgItem::Positional(val)))
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn next_normal(&mut self) -> Result<Option<ArgItem<'p, 'a>>> {
        let Some(arg) = self.iter.next() else {
            return Ok(None);
        };

        if arg == b"--" {
            self.state = State::RestArePositional;
            return self.next();
        }

        if arg.starts_with(b"--") && arg.len() > 2 {
            let arg_body = &arg[2..];
            let (name, maybe_val) = match arg_body.iter().position(|&b| b == b'=') {
                Some(eq_pos) => (&arg_body[..eq_pos], Some(&arg_body[eq_pos + 1..])),
                None => (arg_body, None),
            };

            let Some(param) = self.table.find_long(name) else {
                let name_str = String::from_utf8_lossy(arg).into_owned();
                return Err(Error::UnknownOption(name_str));
            };

            match param.takes_value {
                Values::None => {
                    if maybe_val.is_some() {
                        return Err(Error::DoesntTakeValue("option does not take a value"));
                    }
                    Ok(Some(ArgItem::Option {
                        param,
                        value: None,
                    }))
                }
                Values::One | Values::Many => {
                    let val = match maybe_val {
                        Some(v) => v,
                        None => match self.iter.next() {
                            Some(v) => v,
                            None => return Err(Error::MissingValue("option requires a value")),
                        },
                    };
                    Ok(Some(ArgItem::Option {
                        param,
                        value: Some(val),
                    }))
                }
                Values::OneOptional => {
                    Ok(Some(ArgItem::Option {
                        param,
                        value: maybe_val,
                    }))
                }
            }
        } else if arg.starts_with(b"-") && arg.len() > 1 {
            let first_char = arg[1];
            let Some(param) = self.table.find_short(first_char) else {
                let name_str = String::from_utf8_lossy(arg).into_owned();
                return Err(Error::UnknownOption(name_str));
            };

            if param.takes_value == Values::None {
                if arg.len() > 2 {
                    self.state = State::Chaining { arg, index: 2 };
                }
                Ok(Some(ArgItem::Option {
                    param,
                    value: None,
                }))
            } else {
                let val = if arg.len() > 2 {
                    if arg[2] == b'=' {
                        &arg[3..]
                    } else {
                        &arg[2..]
                    }
                } else {
                    match self.iter.next() {
                        Some(v) => v,
                        None => return Err(Error::MissingValue("option requires a value")),
                    }
                };
                Ok(Some(ArgItem::Option {
                    param,
                    value: Some(val),
                }))
            }
        } else {
            Ok(Some(ArgItem::Positional(arg)))
        }
    }

    fn next_chaining(&mut self, arg: &'a [u8], index: usize) -> Result<Option<ArgItem<'p, 'a>>> {
        let ch = arg[index];
        let Some(param) = self.table.find_short(ch) else {
            let name_str = format!("-{}", ch as char);
            return Err(Error::UnknownOption(name_str));
        };

        let next_idx = index + 1;
        if next_idx >= arg.len() {
            self.state = State::Normal;
        } else {
            self.state = State::Chaining { arg, index: next_idx };
        }

        if param.takes_value == Values::None {
            Ok(Some(ArgItem::Option {
                param,
                value: None,
            }))
        } else {
            self.state = State::Normal;
            let val = if next_idx < arg.len() {
                if arg[next_idx] == b'=' {
                    &arg[next_idx + 1..]
                } else {
                    &arg[next_idx..]
                }
            } else {
                match self.iter.next() {
                    Some(v) => v,
                    None => return Err(Error::MissingValue("option requires a value")),
                }
            };
            Ok(Some(ArgItem::Option {
                param,
                value: Some(val),
            }))
        }
    }
}

