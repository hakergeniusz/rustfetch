//! Shared GNOME desktop-settings reader for the GTK modules. One
//! `gsettings list-recursively` spawn per schema, cached on the Instance so
//! the five GTK modules together cost at most two spawns.

use std::collections::HashMap;

use crate::instance::Instance;

/// Reads one key from a gsettings schema, unquoting string values.
pub fn get(instance: &Instance, schema: &str, key: &str) -> Option<String> {
    {
        let cache = instance.gnome_settings.borrow();
        if let Some(values) = cache.get(schema) {
            return values.get(key).cloned();
        }
    }

    let mut values = HashMap::new();
    if let Ok(output) =
        std::process::Command::new("gsettings").args(["list-recursively", schema]).output()
    {
        // lines look like: "org.gnome.desktop.interface gtk-theme 'Adwaita-dark'"
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut fields = line.splitn(3, ' ');
            let (Some(_schema), Some(name), Some(value)) = (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            values.insert(name.to_string(), value.trim_matches('\'').to_string());
        }
    }

    let value = values.get(key).cloned();
    instance.gnome_settings.borrow_mut().insert(schema.to_string(), values);
    value
}

/// Splits a GTK font name like "Adwaita Sans 11" into ("Adwaita Sans", 11).
pub fn font_name_and_pt(value: &str) -> (&str, Option<u32>) {
    match value.rsplit_once(' ') {
        Some((name, size)) if size.parse::<u32>().is_ok() => (name, size.parse::<u32>().ok()),
        _ => (value, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_name_and_pt_splits_size() {
        assert_eq!(font_name_and_pt("Adwaita Sans 11"), ("Adwaita Sans", Some(11)));
        assert_eq!(font_name_and_pt("Mono 10.5"), ("Mono 10.5", None));
        assert_eq!(font_name_and_pt("Cantarell"), ("Cantarell", None));
    }
}
