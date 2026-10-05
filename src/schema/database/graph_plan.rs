use super::*;
use crate::schema::wire::ContractRef;

pub fn plan_catalog(catalog: &Catalog) -> Result<Vec<DatabasePlan>> {
    catalog.validate()?;
    let plans = catalog.roots.iter().filter(|(_, root)| root.database)
        .map(|(table, root)| {
            if root.database_graphs.is_empty() { return plan(table, &root.history); }
            graph_plan(catalog, table, root)
        }).collect::<Result<Vec<_>>>()?;
    ensure!(!plans.is_empty(), "No database schema roots");
    Ok(plans)
}

fn verify_graph(catalog: &Catalog, graph: &GraphRef, table: &str) -> Result<String> {
    let contract = graph.contract(catalog, &graph.root)?;
    let mut predicates = Vec::new();
    for field in contract.fields.values() {
        let expression = format!("$record.`{}`", identifier(&field.name)?);
        let valid = type_predicate_graph(&field.ty, &expression, 0, Some((catalog, graph)))?;
        let valid = if field.required { valid } else { format!("({expression} = NONE OR {valid})") };
        predicates.push(format!("IF !({valid}) {{ THROW 'Schema record validation failed'; }};"));
    }
    Ok(format!("LET $schema_rows = SELECT * FROM `{table}`; FOR $record IN $schema_rows {{ {} }};", predicates.join("\n")))
}

fn graph_plan(catalog: &Catalog, table: &str, root: &TrackedContract) -> Result<DatabasePlan> {
    identifier(table)?;
    ensure!(root.database_graphs.last() == Some(&catalog.graph_ref(table)?), "Database graph history is stale for {table}");
    let first = &root.database_graphs[0];
    let first_revision = first.contract(catalog, &first.root)?.revision as usize;
    let mut previous = None;
    let mut result = if first_revision > 0 {
        let mut history = root.history.clone();
        history.contracts.truncate(first_revision);
        history.migrations.truncate(first_revision - 1);
        let contract = history.contracts.last().unwrap();
        previous = Some(GraphRef { root: contract.subject.clone(), contracts: [(contract.subject.clone(), ContractRef::new(contract)?)].into() });
        plan(table, &history)?
    } else {
        DatabasePlan { subject: first.root.clone(), table: table.into(), definitions: Vec::new(), statements: Vec::new(), destructive: false }
    };
    for graph in &root.database_graphs {
        let validation = verify_graph(catalog, graph, table)?;
        let statement = if let Some(source) = &previous {
            let mut compiler = Compiler { catalog, source, target: graph, destructive: false, definitions: Vec::new() };
            let expression = compiler.subject(&graph.root, "$record", 0)?;
            result.destructive |= compiler.destructive;
            let before = source.contract(catalog, &source.root)?;
            let after = graph.contract(catalog, &graph.root)?;
            let mut updates = Vec::new();
            for field in after.fields.values() {
                let name = identifier(&field.name)?;
                updates.push(format!("UPDATE $record.id SET `{name}` = $schema_candidate.`{name}`;"));
            }
            for field in before.fields.values() {
                if !after.fields.values().any(|target| target.name == field.name) {
                    updates.push(format!("UPDATE $record.id UNSET `{}`;", identifier(&field.name)?));
                }
            }
            format!("{}\n{}\nLET $schema_rows = SELECT * FROM `{table}`; FOR $record IN $schema_rows {{ LET $schema_candidate = {expression}; {} }};\n{validation}",
                verify_graph(catalog, source, table)?, compiler.definitions.join("\n"), updates.join("\n"))
        } else { validation };
        ensure!(statement.len() <= 1024 * 1024, "Native graph migration exceeds SQL size limit");
        let migrations = graph.contracts.iter().map(|(subject, reference)| {
            let history = catalog.subject_history(subject)?;
            history.migrations.iter().take(reference.revision as usize).map(Migration::fingerprint).collect::<Result<Vec<_>>>()
        }).collect::<Result<Vec<_>>>()?;
        result.definitions.push(format!("surreal-schema-graph-v1:{}:{}:{}", graph.fingerprint()?, serde_json::to_string(&migrations)?, statement));
        result.statements.push(statement);
        previous = Some(graph.clone());
    }
    Ok(result)
}

struct Compiler<'catalog> {
    catalog: &'catalog Catalog,
    source: &'catalog GraphRef,
    target: &'catalog GraphRef,
    destructive: bool,
    definitions: Vec<String>,
}

fn object_expression(entries: Vec<String>, depth: usize) -> String {
    format!("object::from_entries(array::filter([{}], |$schema_entry_{depth}| $schema_entry_{depth}[1] != NONE))", entries.join(","))
}

impl Compiler<'_> {
    fn subject(&mut self, subject: &str, expression: &str, depth: usize) -> Result<String> {
        ensure!(depth <= 64, "Recursive native database schemas exceed the supported depth");
        let target = self.target.contract(self.catalog, subject)?;
        let Some(reference) = self.source.contracts.get(subject) else { return Ok(expression.into()); };
        let history = self.catalog.subject_history(subject)?;
        ensure!(reference.revision <= target.revision, "Database graph revisions go backwards");
        let input_expression = expression;
        let mut expression = "$schema_input".to_owned();
        let mut steps = Vec::new();
        for index in reference.revision as usize..target.revision as usize {
            let source = &history.contracts[index];
            let target = &history.contracts[index + 1];
            let adapter = &history.migrations[index].upgrade;
            let variable = format!("$schema_stage_{depth}_{index}");
            let mut entries = Vec::new();
            for (id, field) in &target.fields {
                let name = if depth == 0 { identifier(&field.name)? } else { member_identifier(&field.name)? };
                let old = source.fields.get(id);
                let input = format!("{variable}.`{}`", old.map(|field| field.name.as_str()).unwrap_or(name));
                let mut value = input.clone();
                if let Some(transform) = adapter.transforms.get(id) {
                    let (transformed, lossy) = transform_expression(transform, &input, depth + 1)?;
                    value = transformed;
                    self.destructive |= lossy;
                }
                if let Some(policy) = adapter.missing.get(id) {
                    match policy {
                        MissingPolicy::Constant { value: default } => value = format!("(IF {input} = NONE {{ {} }} ELSE {{ {value} }})", value_literal(&default)?),
                        MissingPolicy::PreserveAbsent => {},
                        MissingPolicy::Unsupported => bail!("Unsupported nested missing-value policy"),
                    }
                }
                if old.is_some_and(|old| old.name != field.name) && !source.fields.values().any(|old| old.name == field.name) {
                    value = format!("(IF {variable}.`{name}` != NONE AND {variable}.`{name}` != {value} {{ THROW 'Schema rename conflicts with existing target'; }} ELSE {{ {value} }})");
                }
                entries.push(format!("[{}, {value}]", value_literal(&Value::String(field.name.clone()))?));
            }
            self.destructive |= source.fields.keys().any(|id| !target.fields.contains_key(id));
            steps.push(format!("LET {variable} = {expression};"));
            expression = object_expression(entries, depth);
        }
        let variable = format!("$schema_object_{depth}");
        let mut entries = Vec::new();
        for field in target.fields.values() {
            let name = if depth == 0 { identifier(&field.name)? } else { member_identifier(&field.name)? };
            let input = format!("{variable}.`{name}`");
            let value = self.value(&field.ty, &input, depth + 1)?;
            entries.push(format!("[{}, {value}]", value_literal(&Value::String(field.name.clone()))?));
        }
        steps.push(format!("LET {variable} = {expression};"));
        let name = format!("$schema_transform_{}", self.definitions.len());
        self.definitions.push(format!("LET {name} = |$schema_input| {{ {} RETURN {}; }};", steps.join("\n"), object_expression(entries, depth)));
        Ok(format!("{name}({input_expression})"))
    }

    fn value(&mut self, ty: &ValueType, expression: &str, depth: usize) -> Result<String> {
        ensure!(depth <= 64, "Native graph migration exceeds depth limit");
        let adapted = match ty {
            ValueType::Reference { subject } => self.subject(subject, expression, depth + 1)?,
            ValueType::Option { value } => format!("(IF {expression} = NULL {{ NULL }} ELSE {{ {} }})", self.value(value, expression, depth + 1)?),
            ValueType::List { item } => {
                let variable = format!("$schema_element_{depth}");
                format!("array::map({expression}, |{variable}| {})", self.value(item, &variable, depth + 1)?)
            },
            ValueType::Map { value } | ValueType::UuidMap { value } => {
                let variable = format!("$schema_pair_{depth}");
                format!("object::from_entries(array::map(object::entries({expression}), |{variable}| [{variable}[0], {}]))", self.value(value, &format!("{variable}[1]"), depth + 1)?)
            },
            _ => return Ok(expression.into()),
        };
        Ok(format!("(IF {expression} = NONE {{ NONE }} ELSE {{ {adapted} }})"))
    }
}