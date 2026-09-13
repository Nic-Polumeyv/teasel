use super::*;

pub(super) fn plan(plan: &mut Plan) {
	for rule in &mut plan.rules {
		form(&mut rule.form);
		for region in &mut rule.regions {
			value(&mut region.parent);
			value(&mut region.covers);
			if let Some(when) = &mut region.when {
				value(when);
			}
			if let Some(each) = &mut region.each {
				value(&mut each.list);
			}
		}
		for declare in &mut rule.declares {
			value(&mut declare.patterns);
			value(&mut declare.into);
		}
	}
	for row in &mut plan.html.elements {
		value(&mut row.when);
	}
}

fn form(f: &mut Form) {
	match f {
		Form::Seq(items)
		| Form::Choice {
			alternatives: items, ..
		} => items.iter_mut().for_each(form),
		Form::Repeat { body, yield_value, .. } => {
			form(body);
			value(yield_value);
		}
		Form::Read { reader, input, .. } => {
			if let Reader::Test(test) = reader {
				value(test);
			}
			if let Some(input) = input {
				value(input);
			}
		}
		Form::Emit { value: v, .. } => value(v),
	}
}

fn member(expr: &Value) -> Option<ConstantSet> {
	let Value::Compare {
		relation: Relation::Less,
		left,
		right: Some(right),
		..
	} = expr
	else {
		return None;
	};
	if !matches!(left.as_ref(), Value::Constant(Json::Number(n)) if n.as_ref() == "0") {
		return None;
	}
	let Value::Length(list) = right.as_ref() else {
		return None;
	};
	let Value::FlatMap { list, binding, body } = list.as_ref() else {
		return None;
	};
	let Value::Constant(Json::Array(items)) = list.as_ref() else {
		return None;
	};
	let Value::Choose { condition, yes, no } = body.as_ref() else {
		return None;
	};
	let Value::Construct(Construct::Array(yes)) = yes.as_ref() else {
		return None;
	};
	let Value::Construct(Construct::Array(no)) = no.as_ref() else {
		return None;
	};
	if !no.is_empty() || yes.len() != 1 {
		return None;
	}
	let bound =
		|v: &Value| matches!(v, Value::Get { base: Base::Name(name), path } if name == binding && path.is_empty());
	if !bound(&yes[0]) {
		return None;
	}
	let Value::Compare {
		relation: Relation::Equal,
		left,
		right: Some(right),
		..
	} = condition.as_ref()
	else {
		return None;
	};
	let needle = if bound(left) {
		right.clone()
	} else if bound(right) {
		left.clone()
	} else {
		return None;
	};
	let strings = items
		.iter()
		.map(|v| if let Json::String(v) = v { Some(v.clone()) } else { None })
		.collect::<Option<_>>()?;
	Some(ConstantSet { needle, strings })
}

fn value(v: &mut Value) {
	let compiled_set = member(v);
	match v {
		Value::Constant(_) => return,
		Value::Get {
			base: Base::Value(base),
			..
		} => value(base),
		Value::Get { .. } => {}
		Value::Compare { left, right, set, .. } => {
			value(left);
			if let Some(right) = right {
				value(right);
			}
			*set = compiled_set;
		}
		Value::Choose { condition, yes, no } => {
			value(condition);
			value(yes);
			value(no);
			if let Value::Constant(Json::Bool(test)) = condition.as_ref() {
				*v = if *test { *yes.clone() } else { *no.clone() };
				return;
			}
		}
		Value::FlatMap { list, body, .. } => {
			value(list);
			value(body);
		}
		Value::Length(list) => value(list),
		Value::At { list, index } => {
			value(list);
			value(index);
		}
		Value::Construct(Construct::Array(items)) => items.iter_mut().for_each(value),
		Value::Construct(Construct::Record { fields, span, .. }) => {
			fields.values_mut().for_each(value);
			value(span);
		}
	}
	if let Some(folded) = constant(v) {
		*v = Value::Constant(folded);
	}
}

fn constant(v: &Value) -> Option<Json> {
	Some(match v {
		Value::Constant(v) => v.clone(),
		Value::Get {
			base: Base::Value(base),
			path,
		} => {
			let mut v = constant(base)?;
			for part in path {
				v = match (v, part) {
					(Json::Object(fields), Path::Name(key)) => fields.get(key)?.clone(),
					(Json::Array(items), Path::Index(index)) => items.get(*index)?.clone(),
					_ => return None,
				};
			}
			v
		}
		Value::Compare {
			relation, left, right, ..
		} => {
			let left = constant(left)?;
			Json::Bool(match relation {
				Relation::Present => true,
				Relation::Equal => left == constant(right.as_ref()?)?,
				Relation::Less => {
					let (Json::Number(a), Json::Number(b)) = (left, constant(right.as_ref()?)?) else {
						return None;
					};
					a.parse::<f64>().ok()? < b.parse::<f64>().ok()?
				}
			})
		}
		Value::Length(list) => {
			let Json::Array(list) = constant(list)? else {
				return None;
			};
			Json::Number(list.len().to_string().into())
		}
		Value::At { list, index } => {
			let (Json::Array(list), Json::Number(index)) = (constant(list)?, constant(index)?) else {
				return None;
			};
			list.get(index.parse::<usize>().ok()?)?.clone()
		}
		Value::Construct(Construct::Array(items)) => Json::Array(items.iter().map(constant).collect::<Option<_>>()?),
		_ => return None,
	})
}
