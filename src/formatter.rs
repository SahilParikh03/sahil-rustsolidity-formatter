use regex::Regex;
use std::process::Command;
use tempfile::NamedTempFile;
use std::io::Write;
use std::fs;
use anyhow::{Result, Context};

/// Run forge fmt as a foundation, return original code on failure
pub fn run_forge_fmt(code: &str) -> Result<String> {
    // Create a temporary file
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(code.as_bytes())?;
    let temp_path = temp_file.path();
    
    // Run forge fmt
    let output = Command::new("forge")
        .args(["fmt", temp_path.to_str().unwrap()])
        .output();
    
    match output {
        Ok(result) if result.status.success() => {
            // Read the formatted content
            let formatted = fs::read_to_string(temp_path)
                .context("Failed to read formatted file")?;
            Ok(formatted)
        }
        _ => {
            // Return original code on failure
            Ok(code.to_string())
        }
    }
}

/// Preserve original trailing newline behavior
pub fn preserve_trailing_newline(original: &str, formatted: &str) -> String {
    if !original.ends_with('\n') && formatted.ends_with('\n') {
        formatted.trim_end_matches('\n').to_string()
    } else if original.ends_with('\n') && !formatted.ends_with('\n') {
        format!("{}\n", formatted)
    } else {
        formatted.to_string()
    }
}

/// Find groups of consecutive lines matching a pattern
fn find_consecutive_matching_lines<F>(lines: &[String], pattern: &Regex, filter_func: Option<F>) -> Vec<Vec<usize>>
where
    F: Fn(&str) -> bool,
{
    let mut matching_lines = Vec::new();
    
    for (line_idx, line) in lines.iter().enumerate() {
        if pattern.is_match(line) && filter_func.as_ref().map_or(true, |f| f(line)) {
            matching_lines.push(line_idx);
        }
    }
    
    // Group consecutive lines
    let mut groups = Vec::new();
    let mut current_group = Vec::new();
    
    for &line_idx in &matching_lines {
        if current_group.is_empty() || line_idx == current_group.last().unwrap() + 1 {
            current_group.push(line_idx);
        } else {
            if current_group.len() > 1 {
                groups.push(current_group);
            }
            current_group = vec![line_idx];
        }
    }
    
    if current_group.len() > 1 {
        groups.push(current_group);
    }
    
    groups
}

/// Generic helper to align consecutive lines by capture groups
fn align_by_capture_groups<F>(
    lines: Vec<String>,
    pattern: &Regex,
    format_func: F,
    filter_func: Option<fn(&str) -> bool>,
) -> Vec<String>
where
    F: Fn(&str, &regex::Captures, usize) -> String,
{
    let mut result = lines.clone();
    let groups = find_consecutive_matching_lines(&lines, pattern, filter_func);
    
    for group in groups {
        if group.len() <= 1 {
            continue;
        }
        
        // Calculate max length for alignment
        let mut max_length = 0;
        let mut group_matches = Vec::new();
        
        for &line_idx in &group {
            let line = &lines[line_idx];
            if let Some(captures) = pattern.captures(line) {
                // Calculate max_length before moving captures
                if let Some(first_group) = captures.get(1) {
                    max_length = max_length.max(first_group.as_str().len());
                }
                group_matches.push((line_idx, captures));
            }
        }
        
        // Apply formatting
        for (line_idx, captures) in group_matches {
            result[line_idx] = format_func(&lines[line_idx], &captures, max_length);
        }
    }
    
    result
}

/// Convert uint256 to uint for shorter syntax
pub fn convert_uint256_to_uint(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            if line.trim_start().starts_with("//") {
                line
            } else {
                line.replace("uint256", "uint")
            }
        })
        .collect()
}

/// Format and align import statements
pub fn format_import_statements(lines: Vec<String>) -> Vec<String> {
    let import_pattern = Regex::new(r"^(\s*import\s+.+?)\s+from\s+(.+)$").unwrap();
    
    align_by_capture_groups(
        lines,
        &import_pattern,
        |line: &str, captures: &regex::Captures, max_length: usize| {
            let import_part = captures.get(1).unwrap().as_str().trim();
            let from_part = captures.get(2).unwrap().as_str().trim();
            let indent = &line[..line.len() - line.trim_start().len()];
            
            let padding_needed = max_length - import_part.len();
            let padding = " ".repeat(padding_needed + 1); // +1 for separation
            
            format!("{}{}{}from {}", indent, import_part, padding, from_part)
        },
        None,
    )
}

/// Format and align variable declarations
pub fn format_variable_declarations(lines: Vec<String>) -> Vec<String> {
    let var_pattern = Regex::new(r"^\s*(uint\d*|address|bool|bytes\d*|string)\s+(public|private|internal)\s+").unwrap();
    
    let filter_func = |line: &str| !line.trim_start().starts_with("//");
    
    align_by_capture_groups(
        lines,
        &var_pattern,
        |line: &str, captures: &regex::Captures, max_length: usize| {
            let type_name = captures.get(1).unwrap().as_str();
            let padding_needed = max_length - type_name.len();
            let padding = " ".repeat(padding_needed);
            
            // Create a new regex pattern inside the closure to avoid borrowing issues
            let pattern = Regex::new(r"^\s*(uint\d*|address|bool|bytes\d*|string)\s+(public|private|internal)\s+").unwrap();
            pattern.replace(line, |caps: &regex::Captures| {
                let start = &line[..caps.get(1).unwrap().start()];
                let type_part = caps.get(1).unwrap().as_str();
                let visibility = caps.get(2).unwrap().as_str();
                format!("{}{}{} {} ", start, type_part, padding, visibility)
            }).into_owned()
        },
        Some(filter_func),
    )
}

/// Format function declarations with proper multiline style
pub fn format_function_declarations(lines: Vec<String>) -> Vec<String> {
    let single_line_func_pattern = Regex::new(r"^(\s*function\s+\w+)\(([^)]*)\)\s*(.+?)\s*\{?\s*$").unwrap();
    let func_sig_pattern = Regex::new(r"^(\s*function\s+\w+)\(([^)]*)\)\s*$").unwrap();
    
    let mut result = lines;
    let mut i = 0;
    
    while i < result.len() {
        let line = &result[i].clone();
        
        // Pattern 1: Single-line function with visibility
        if let Some(captures) = single_line_func_pattern.captures(line) {
            let after_params = captures.get(3).map_or("", |m| m.as_str());
            if ["external", "public", "private", "internal"].iter().any(|vis| after_params.contains(vis)) {
                let new_lines = convert_function_to_multiline(line, &captures);
                result.splice(i..i+1, new_lines.iter().cloned());
                i += new_lines.len();
                continue;
            }
        }
        
        // Pattern 2: Function signature with parameters on same line
        if let Some(captures) = func_sig_pattern.captures(line) {
            let params_str = captures.get(2).map_or("", |m| m.as_str()).trim();
            let params: Vec<&str> = if params_str.is_empty() {
                Vec::new()
            } else {
                params_str.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()).collect()
            };
            
            if params.len() > 2 {
                let new_lines = reformat_function_parameters(line, &captures);
                result.splice(i..i+1, new_lines.iter().cloned());
                i += new_lines.len();
                continue;
            }
        }
        
        i += 1;
    }
    
    result
}

/// Convert single-line function to proper multiline format
fn convert_function_to_multiline(line: &str, captures: &regex::Captures) -> Vec<String> {
    let func_name_part = captures.get(1).unwrap().as_str();
    let params_str = captures.get(2).map_or("", |m| m.as_str()).trim();
    let mut after_params = captures.get(3).map_or("", |m| m.as_str()).trim();
    
    if after_params.ends_with('{') {
        after_params = &after_params[..after_params.len()-1].trim();
    }
    
    let indent = &line[..line.len() - line.trim_start().len()];
    let mut new_lines = Vec::new();
    
    let params: Vec<&str> = if params_str.is_empty() {
        Vec::new()
    } else {
        params_str.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()).collect()
    };
    
    if params.len() > 2 {
        new_lines.push(format!("{}(", func_name_part));
        for (i, param) in params.iter().enumerate() {
            if i == params.len() - 1 {
                new_lines.push(format!("{}    {}", indent, param));
            } else {
                new_lines.push(format!("{}    {},", indent, param));
            }
        }
        new_lines.push(format!("{})", indent));
    } else {
        new_lines.push(format!("{}({})", func_name_part, params_str));
    }
    
    if !after_params.is_empty() {
        let parts: Vec<&str> = after_params.split_whitespace().collect();
        for part in parts {
            new_lines.push(format!("{}    {}", indent, part));
        }
    }
    
    new_lines.push(format!("{}{{", indent));
    new_lines
}

/// Reformat function parameters to be on separate lines when >2 params
fn reformat_function_parameters(line: &str, captures: &regex::Captures) -> Vec<String> {
    let func_name_part = captures.get(1).unwrap().as_str();
    let params_str = captures.get(2).map_or("", |m| m.as_str()).trim();
    
    let params: Vec<&str> = params_str.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()).collect();
    let indent = &line[..line.len() - line.trim_start().len()];
    
    let mut new_lines = vec![format!("{}(", func_name_part)];
    
    for (i, param) in params.iter().enumerate() {
        if i == params.len() - 1 {
            new_lines.push(format!("{}    {}", indent, param));
        } else {
            new_lines.push(format!("{}    {},", indent, param));
        }
    }
    
    new_lines.push(format!("{})", indent));
    new_lines
}

/// Format constructor declarations with aligned parameters
pub fn format_constructors(lines: Vec<String>) -> Vec<String> {
    let mut result = lines;
    let mut i = 0;
    
    while i < result.len() {
        let line = &result[i].clone();
        
        if line.contains("constructor(") && !line.trim_start().starts_with("//") {
            let constructor_start = i;
            let mut constructor_lines = Vec::new();
            let mut j = i;
            
            // Collect all lines of the constructor declaration
            while j < result.len() {
                constructor_lines.push(j);
                let current_line = &result[j];
                
                // Find matching closing parenthesis
                let open_count = current_line.matches('(').count();
                let close_count = current_line.matches(')').count();
                
                if current_line.contains(')') && open_count <= close_count {
                    break;
                }
                j += 1;
            }
            
            // Only process if we have multiple parameter lines
            if constructor_lines.len() > 2 {
                let new_lines = format_constructor_params(&result, &constructor_lines);
                result.splice(constructor_start..constructor_start + constructor_lines.len(), new_lines.iter().cloned());
                i = constructor_start + new_lines.len();
                continue;
            }
        }
        
        i += 1;
    }
    
    result
}

/// Format constructor parameters with proper alignment
fn format_constructor_params(lines: &[String], constructor_lines: &[usize]) -> Vec<String> {
    let mut new_lines = vec![lines[constructor_lines[0]].clone()];
    
    // Extract parameters from middle lines
    let mut params = Vec::new();
    for &line_idx in &constructor_lines[1..constructor_lines.len()-1] {
        let param_line = lines[line_idx].trim();
        if !param_line.is_empty() && !param_line.starts_with("//") {
            let param = param_line.trim_end_matches(',');
            params.push(param.to_string());
        }
    }
    
    // Handle last line parameter
    if constructor_lines.len() > 1 {
        let last_line = &lines[constructor_lines[constructor_lines.len()-1]];
        if let Some(paren_pos) = last_line.find(')') {
            let param_part = last_line[..paren_pos].trim();
            if !param_part.is_empty() {
                let param = param_part.trim_end_matches(',');
                params.push(param.to_string());
            }
        }
    }
    
    // Calculate alignment
    let mut max_type_length = 0;
    let mut max_modifier_length = 0;
    let mut param_parts = Vec::new();
    
    for param in &params {
        if param.is_empty() {
            continue;
        }
        
        let parts: Vec<&str> = param.split_whitespace().collect();
        if parts.len() >= 2 {
            if parts.len() >= 3 && ["memory", "storage", "calldata"].contains(&parts[parts.len()-2]) {
                // Type with memory modifier
                let type_part = parts[..parts.len()-2].join(" ");
                let memory_part = parts[parts.len()-2];
                let name_part = parts[parts.len()-1];
                
                param_parts.push((type_part.clone(), Some(memory_part.to_string()), name_part.to_string()));
                max_type_length = max_type_length.max(type_part.len());
                max_modifier_length = max_modifier_length.max(memory_part.len());
            } else {
                // Simple type
                let mut type_part = parts[0].to_string();
                if type_part == "uint256" {
                    type_part = "uint".to_string();
                }
                let name_part = parts[1..].join(" ");
                
                param_parts.push((type_part.clone(), None, name_part));
                max_type_length = max_type_length.max(type_part.len());
            }
        }
    }
    
    // Get indent from first line
    let indent = &lines[constructor_lines[0]][..lines[constructor_lines[0]].len() - lines[constructor_lines[0]].trim_start().len()];
    
    // Add aligned parameters
    for (idx, (type_part, memory_part, name_part)) in param_parts.iter().enumerate() {
        let aligned_param = if let Some(memory) = memory_part {
            let type_padding = " ".repeat(max_type_length - type_part.len() + 1);
            let modifier_padding = " ".repeat(max_modifier_length - memory.len() + 1);
            format!("{}{}{}{}{}", type_part, type_padding, memory, modifier_padding, name_part)
        } else {
            let type_padding = " ".repeat(max_type_length - type_part.len() + max_modifier_length + 2);
            format!("{}{}{}", type_part, type_padding, name_part)
        };
        
        if idx < param_parts.len() - 1 {
            new_lines.push(format!("{}    {},", indent, aligned_param));
        } else {
            new_lines.push(format!("{}    {}", indent, aligned_param));
        }
    }
    
    // Add closing line
    let last_orig_line = &lines[constructor_lines[constructor_lines.len()-1]];
    if let Some(paren_pos) = last_orig_line.find(')') {
        let extra_after_paren = &last_orig_line[paren_pos+1..];
        let extra = if extra_after_paren.trim() == "{" {
            "  {"
        } else {
            extra_after_paren
        };
        new_lines.push(format!("{}){}", indent, extra));
    }
    
    new_lines
}

/// Format require statements with aligned conditions and error messages
pub fn format_require_statements(lines: Vec<String>) -> Vec<String> {
    let mut result = lines.clone();
    
    // Find groups of consecutive require statements
    let mut require_groups = Vec::new();
    let mut current_group = Vec::new();
    
    for (i, line) in lines.iter().enumerate() {
        let stripped = line.trim();
        if stripped.starts_with("require(") && !line.trim_start().starts_with("//") {
            current_group.push(i);
        } else {
            if current_group.len() > 1 {
                require_groups.push(current_group);
            }
            current_group = Vec::new();
        }
    }
    
    if current_group.len() > 1 {
        require_groups.push(current_group);
    }
    
    // Process each group
    for group in require_groups {
        let formatted_requires = format_require_group(&lines, &group);
        for (i, &line_idx) in group.iter().enumerate() {
            if i < formatted_requires.len() {
                result[line_idx] = formatted_requires[i].clone();
            }
        }
    }
    
    result
}

/// Format a group of require statements
fn format_require_group(lines: &[String], group: &[usize]) -> Vec<String> {
    let mut require_data = Vec::new();
    let mut max_condition_length = 0;
    
    for &line_idx in group {
        let line = &lines[line_idx];
        let indent = &line[..line.len() - line.trim_start().len()];
        let stripped = line.trim();
        
        if let Some((condition, error)) = parse_require_statement(stripped) {
            let aligned_condition = align_require_condition(&condition);
            max_condition_length = max_condition_length.max(aligned_condition.len());
            require_data.push((indent.to_string(), aligned_condition, error));
        }
    }
    
    // Generate aligned require statements
    require_data
        .into_iter()
        .map(|(indent, condition, error)| {
            let padding_needed = max_condition_length - condition.len();
            let padding = " ".repeat(padding_needed);
            format!("{}require({},{} {});", indent, condition, padding, error)
        })
        .collect()
}

/// Parse require statement to extract condition and error
fn parse_require_statement(require_line: &str) -> Option<(String, String)> {
    if !require_line.starts_with("require(") {
        return None;
    }
    
    // Find the content between require( and )
    let start = 8; // "require(".len()
    let mut paren_count = 1;
    let mut end = start;
    
    for (i, ch) in require_line[start..].char_indices() {
        match ch {
            '(' => paren_count += 1,
            ')' => {
                paren_count -= 1;
                if paren_count == 0 {
                    end = start + i;
                    break;
                }
            }
            _ => {}
        }
    }
    
    if end == start {
        return None;
    }
    
    let content = &require_line[start..end];
    
    // Find the last comma that separates condition from error
    let mut comma_positions = Vec::new();
    let mut paren_depth = 0;
    
    for (i, ch) in content.char_indices() {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            ',' if paren_depth == 0 => comma_positions.push(i),
            _ => {}
        }
    }
    
    if let Some(&last_comma) = comma_positions.last() {
        let condition = content[..last_comma].trim().to_string();
        let error = content[last_comma + 1..].trim().to_string();
        Some((condition, error))
    } else {
        None
    }
}

/// Align require condition by operator
fn align_require_condition(condition: &str) -> String {
    let operators = ["<=", ">=", "==", "!=", "<", ">", "&&", "||"];
    
    for op in &operators {
        if let Some(pos) = find_operator_position(condition, op) {
            let left = condition[..pos].trim();
            let right = condition[pos + op.len()..].trim();
            return format!("{} {} {}", left, op, right);
        }
    }
    
    condition.to_string()
}

/// Find operator position not inside parentheses
fn find_operator_position(condition: &str, operator: &str) -> Option<usize> {
    let mut paren_depth = 0;
    let operator_len = operator.len();
    
    for i in 0..=condition.len().saturating_sub(operator_len) {
        // Update paren depth for characters before position i
        for ch in condition[..i].chars() {
            match ch {
                '(' => paren_depth += 1,
                ')' => paren_depth -= 1,
                _ => {}
            }
        }
        
        if paren_depth == 0 && condition[i..].starts_with(operator) {
            return Some(i);
        }
    }
    
    None
}

/// Format struct field assignments with aligned colons
pub fn format_struct_assignments(lines: Vec<String>) -> Vec<String> {
    let mut result = lines.clone();
    let mut i = 0;
    
    while i < result.len() {
        let line = &result[i].clone();
        
        if line.contains('{') && line.contains('(') && !line.trim_start().starts_with("//") {
            if let Some(brace_pos) = line.find('{') {
                let before_brace = &line[..brace_pos].trim();
                if before_brace.ends_with('(') || before_brace.contains("= ") {
                    let formatted_lines = format_struct_fields(&result, i);
                    if formatted_lines.len() > 1 {
                        result.splice(i..i + formatted_lines.len(), formatted_lines.iter().cloned());
                        i += formatted_lines.len();
                        continue;
                    }
                }
            }
        }
        
        i += 1;
    }
    
    result
}

/// Format struct fields with aligned colons
fn format_struct_fields(lines: &[String], start_idx: usize) -> Vec<String> {
    let mut struct_lines = Vec::new();
    let mut brace_count = 0;
    let mut j = start_idx;
    
    // Collect all lines of the struct
    while j < lines.len() {
        let current_line = &lines[j];
        struct_lines.push(current_line.clone());
        
        brace_count += current_line.matches('{').count() as i32;
        brace_count -= current_line.matches('}').count() as i32;
        
        if brace_count == 0 && current_line.contains('}') {
            break;
        }
        j += 1;
    }
    
    if struct_lines.len() <= 2 {
        return struct_lines;
    }
    
    // Parse field lines
    let mut field_data = Vec::new();
    let mut max_field_name_length = 0;
    
    for (idx, line) in struct_lines.iter().enumerate() {
        if idx == 0 || idx == struct_lines.len() - 1 {
            continue; // Skip opening and closing lines
        }
        
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with("//") {
            continue;
        }
        
        if let Some(colon_pos) = stripped.find(':') {
            let field_name = stripped[..colon_pos].trim();
            let mut field_value = stripped[colon_pos + 1..].trim();
            
            let has_comma = field_value.ends_with(',');
            if has_comma {
                field_value = &field_value[..field_value.len() - 1].trim();
            }
            
            field_data.push((idx, field_name.to_string(), field_value.to_string(), has_comma));
            max_field_name_length = max_field_name_length.max(field_name.len());
        }
    }
    
    // Rebuild with aligned fields
    let mut result = struct_lines.clone();
    for (idx, field_name, field_value, has_comma) in field_data {
        let indent = &struct_lines[idx][..struct_lines[idx].len() - struct_lines[idx].trim_start().len()];
        let padding = " ".repeat(max_field_name_length - field_name.len());
        let comma = if has_comma { "," } else { "" };
        result[idx] = format!("{}{}:{} {}{}", indent, field_name, padding, field_value, comma);
    }
    
    result
}

/// Format variable assignments with aligned = operators and storage/memory keywords
pub fn format_variable_assignments(lines: Vec<String>) -> Vec<String> {
    let assignment_groups = find_assignment_groups(&lines);
    let mut result = lines.clone();
    
    for group in assignment_groups {
        if group.len() <= 1 {
            continue;
        }
        
        let formatted_assignments = format_assignment_group(&lines, &group);
        for (i, &(line_idx, _)) in group.iter().enumerate() {
            if i < formatted_assignments.len() {
                result[line_idx] = formatted_assignments[i].clone();
            }
        }
    }
    
    result
}

/// Find consecutive groups of assignment statements
fn find_assignment_groups(lines: &[String]) -> Vec<Vec<(usize, (String, String))>> {
    let mut groups = Vec::new();
    let mut current_group = Vec::new();
    
    for (i, line) in lines.iter().enumerate() {
        let stripped = line.trim();
        
        if stripped.contains('=')
            && !stripped.starts_with("//")
            && !stripped.contains("pragma")
            && !stripped.contains("import")
            && !stripped.contains("require(")
        {
            let parts: Vec<&str> = stripped.splitn(2, '=').collect();
            if parts.len() == 2 {
                let var_part = parts[0].trim();
                let value_part = parts[1].trim();
                
                // Check it's not a comparison operator
                if !["==", "!=", "<=", ">="].iter().any(|op| var_part.contains(op)) {
                    current_group.push((i, (var_part.to_string(), value_part.to_string())));
                    continue;
                }
            }
        }
        
        if !current_group.is_empty() {
            groups.push(current_group);
            current_group = Vec::new();
        }
    }
    
    if !current_group.is_empty() {
        groups.push(current_group);
    }
    
    groups
}

/// Format a group of assignment statements
fn format_assignment_group(lines: &[String], group: &[(usize, (String, String))]) -> Vec<String> {
    // For simplicity, use basic alignment for now
    let mut max_var_length = 0;
    
    for (_, (var_part, _)) in group {
        max_var_length = max_var_length.max(var_part.len());
    }
    
    group
        .iter()
        .map(|(line_idx, (var_part, value_part))| {
            let original_line = &lines[*line_idx];
            let indent = &original_line[..original_line.len() - original_line.trim_start().len()];
            let padding_needed = max_var_length - var_part.len();
            let padding = " ".repeat(padding_needed);
            
            format!("{}{}{} = {}", indent, var_part, padding, value_part)
        })
        .collect()
}

/// Add double space before opening brace in function/constructor declarations
pub fn add_double_space_before_brace(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            if line.trim().ends_with(") {") {
                line.replacen(") {", ")  {", 1)
            } else {
                line
            }
        })
        .collect()
} 
