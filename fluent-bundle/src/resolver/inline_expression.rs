use super::scope::Scope;
use super::{ResolveValue, ResolverError, WriteValue};

use std::borrow::Borrow;
use std::fmt;

use fluent_syntax::ast;
use fluent_syntax::unicode::{unescape_unicode, unescape_unicode_to_string};

use crate::entry::GetEntry;
use crate::memoizer::MemoizerKind;
use crate::resource::FluentResource;
use crate::types::FluentValue;

impl<'p> WriteValue for ast::InlineExpression<&'p str> {
    fn write<'scope, 'errors, W, R, M: MemoizerKind>(
        &'scope self,
        w: &mut W,
        scope: &mut Scope<'scope, 'errors, R, M>,
    ) -> fmt::Result
    where
        W: fmt::Write,
        R: Borrow<FluentResource>,
    {
        match self {
            Self::StringLiteral { value } => unescape_unicode(w, value),
            Self::MessageReference { id, attribute } => scope
                .bundle
                .get_entry_message(id.name)
                .and_then(|msg| {
                    if let Some(attr) = attribute {
                        msg.attributes.iter().find_map(|a| {
                            if a.id.name == attr.name {
                                Some(scope.track(w, &a.value, self))
                            } else {
                                None
                            }
                        })
                    } else {
                        msg.value.as_ref().map(|value| scope.track(w, value, self))
                    }
                })
                .unwrap_or_else(|| scope.write_ref_error(w, self)),
            Self::NumberLiteral { value } => FluentValue::try_number(*value).write(w, scope),
            Self::TermReference {
                id,
                attribute,
                arguments,
            } => {
                let (_, resolved_named_args) = scope.get_arguments(arguments);

                scope.local_args = Some(resolved_named_args);
                let result = scope
                    .bundle
                    .get_entry_term(id.name)
                    .and_then(|term| {
                        if let Some(attr) = attribute {
                            term.attributes.iter().find_map(|a| {
                                if a.id.name == attr.name {
                                    Some(scope.track(w, &a.value, self))
                                } else {
                                    None
                                }
                            })
                        } else {
                            Some(scope.track(w, &term.value, self))
                        }
                    })
                    .unwrap_or_else(|| scope.write_ref_error(w, self));
                scope.local_args = None;
                result
            }
            Self::FunctionReference { id, arguments } => {
                let (resolved_positional_args, resolved_named_args) =
                    scope.get_arguments(arguments);

                let func = scope.bundle.get_entry_function(id.name);

                if let Some(func) = func {
                    let result = func(resolved_positional_args.as_slice(), &resolved_named_args);
                    if let FluentValue::Error = result {
                        self.write_error(w)
                    } else {
                        w.write_str(&result.as_string(scope))
                    }
                } else {
                    scope.write_ref_error(w, self)
                }
            }
            Self::VariableReference { id } => {
                let args = scope.local_args.as_ref().or(scope.args);

                if let Some(arg) = args.and_then(|args| args.get(id.name)) {
                    arg.write(w, scope)
                } else {
                    if scope.local_args.is_none() {
                        scope.add_error(ResolverError::Reference(self.resolve_error()));
                    }
                    w.write_char('{')?;
                    self.write_error(w)?;
                    w.write_char('}')
                }
            }
            Self::Placeable { expression } => expression.write(w, scope),
        }
    }

    fn write_error<W>(&self, w: &mut W) -> fmt::Result
    where
        W: fmt::Write,
    {
        match self {
            Self::MessageReference {
                id,
                attribute: Some(attribute),
            } => write!(w, "{}.{}", id.name, attribute.name),
            Self::MessageReference {
                id,
                attribute: None,
            } => w.write_str(id.name),
            Self::TermReference {
                id,
                attribute: Some(attribute),
                ..
            } => write!(w, "-{}.{}", id.name, attribute.name),
            Self::TermReference {
                id,
                attribute: None,
                ..
            } => write!(w, "-{}", id.name),
            Self::FunctionReference { id, .. } => write!(w, "{}()", id.name),
            Self::VariableReference { id } => write!(w, "${}", id.name),
            _ => unreachable!(),
        }
    }
}

impl<'p> ResolveValue for ast::InlineExpression<&'p str> {
    fn resolve<'source, 'errors, R, M: MemoizerKind>(
        &'source self,
        scope: &mut Scope<'source, 'errors, R, M>,
    ) -> FluentValue<'source>
    where
        R: Borrow<FluentResource>,
    {
        match self {
            Self::StringLiteral { value } => unescape_unicode_to_string(value).into(),
            Self::NumberLiteral { value } => FluentValue::try_number(*value),
            Self::VariableReference { id } => {
                let args = scope.local_args.as_ref().or(scope.args);

                if let Some(arg) = args.and_then(|args| args.get(id.name)) {
                    arg.clone()
                } else {
                    if scope.local_args.is_none() {
                        scope.add_error(ResolverError::Reference(self.resolve_error()));
                    }
                    FluentValue::Error
                }
            }
            _ => {
                let mut result = String::new();
                self.write(&mut result, scope).expect("Failed to write");
                result.into()
            }
        }
    }

    fn resolve_error(&self) -> String {
        match self {
            Self::MessageReference {
                attribute: None, ..
            } => {
                let mut error = String::from("Unknown message: ");
                self.write_error(&mut error)
                    .expect("Failed to write to String.");
                error
            }
            Self::MessageReference { .. } => {
                let mut error = String::from("Unknown attribute: ");
                self.write_error(&mut error)
                    .expect("Failed to write to String.");
                error
            }
            Self::VariableReference { .. } => {
                let mut error = String::from("Unknown variable: ");
                self.write_error(&mut error)
                    .expect("Failed to write to String.");
                error
            }
            Self::TermReference {
                attribute: None, ..
            } => {
                let mut error = String::from("Unknown term: ");
                self.write_error(&mut error)
                    .expect("Failed to write to String.");
                error
            }
            Self::TermReference { .. } => {
                let mut error = String::from("Unknown attribute: ");
                self.write_error(&mut error)
                    .expect("Failed to write to String.");
                error
            }
            Self::FunctionReference { .. } => {
                let mut error = String::from("Unknown function: ");
                self.write_error(&mut error)
                    .expect("Failed to write to String.");
                error
            }
            _ => unreachable!(),
        }
    }
}