use crate::model::Server;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Snippet {
    pub key: String,
    pub value: String,
}
#[derive(Default)]
pub struct Context<'a> {
    pub cluster: &'a str,
    pub agent: &'a str,
    pub task: &'a str,
    pub host: &'a str,
    pub cwd: &'a str,
}
#[derive(Clone)]
pub struct Variable {
    pub token: String,
    pub label: String,
    pub value: String,
    pub nested: bool,
}
pub fn variables(servers: &[Server], snippets: &[Snippet], ctx: &Context<'_>) -> Vec<Variable> {
    let mut vars = vec![];
    for (key, label, value) in [
        ("имя_кластера", "Название кластера", ctx.cluster),
        ("имя_агента", "Имя агента", ctx.agent),
        ("задача", "Текущая задача", ctx.task),
        ("хост", "Адрес выбранного хоста", ctx.host),
        ("рабочая_папка", "Рабочая папка агента", ctx.cwd),
    ] {
        vars.push(Variable {
            token: format!("[{key}]"),
            label: label.into(),
            value: value.into(),
            nested: false,
        });
    }
    vars.push(Variable {
        token: "[дата]".into(),
        label: "Текущая дата".into(),
        value: chrono::Local::now().format("%Y-%m-%d").to_string(),
        nested: false,
    });
    for server in servers {
        let duplicate = servers.iter().filter(|s| s.name == server.name).count() > 1;
        let name = if duplicate {
            format!(
                "{}#{}",
                server.name,
                server.id.chars().take(8).collect::<String>()
            )
        } else {
            server.name.clone()
        };
        let data = format!(
            "Сервер: {}\nАдрес: {}\nПорт: {}\nПользователь: {}\nРабочая папка: {}",
            server.name, server.host, server.port, server.user, server.cwd
        );
        for (key, label, value) in [
            ("данные_айпи", "IP / домен", server.host.clone()),
            ("данные_порта", "SSH-порт", server.port.to_string()),
            ("данные_пользователя", "Пользователь", server.user.clone()),
            ("данные_папки", "Рабочая папка", server.cwd.clone()),
            ("данные_сервера", "Профиль целиком", data),
        ] {
            vars.push(Variable {
                token: format!("[{key}_({name})]"),
                label: format!("{} · {label}", server.name),
                value: value.clone(),
                nested: false,
            });
            vars.push(Variable {
                token: format!("[{key}_(#{})]", server.id),
                label: format!("{} · {label} · ID", server.name),
                value,
                nested: false,
            });
        }
    }
    for snippet in snippets {
        if !snippet.key.trim().is_empty() {
            vars.push(Variable {
                token: format!("[{}]", snippet.key.trim()),
                label: format!("Моя вставка · {}", snippet.key.trim()),
                value: snippet.value.clone(),
                nested: true,
            });
        }
    }
    vars
}
pub fn expand(text: &str, vars: &[Variable]) -> Result<String, String> {
    let mut map = BTreeMap::new();
    let mut duplicate = HashSet::new();
    for v in vars {
        if map.insert(v.token.as_str(), v).is_some() {
            duplicate.insert(v.token.as_str());
        }
    }
    fn resolve(
        text: &str,
        map: &BTreeMap<&str, &Variable>,
        duplicate: &HashSet<&str>,
        stack: &mut Vec<String>,
    ) -> Result<String, String> {
        if stack.len() > 8 {
            return Err("Слишком много вложенных вставок.".into());
        }
        let mut out = String::new();
        let mut at = 0;
        while at < text.len() {
            if !text[at..].starts_with('[') {
                let n = text[at..].find('[').map(|n| at + n).unwrap_or(text.len());
                out.push_str(&text[at..n]);
                at = n;
                continue;
            }
            let found = map
                .iter()
                .filter(|(key, _)| text[at..].starts_with(**key))
                .max_by_key(|(key, _)| key.len());
            if let Some((key, var)) = found {
                if duplicate.contains(key) {
                    return Err(format!("Вставка {key} неоднозначна: укажи отдельное имя профиля или выбери ключ из списка."));
                }
                if stack.iter().any(|k| k == key) {
                    return Err(format!("Циклическая вставка {key}."));
                }
                if var.nested {
                    stack.push((*key).into());
                    out.push_str(&resolve(&var.value, map, duplicate, stack)?);
                    stack.pop();
                } else {
                    out.push_str(&var.value);
                }
                at += key.len();
            } else if text[at..].starts_with("[данные_") {
                let end = text[at..]
                    .find(']')
                    .map(|n| at + n + 1)
                    .unwrap_or(text.len());
                return Err(format!(
                    "Профиль для {} не найден. Выбери имя из сохранённых подключений Relay.",
                    &text[at..end]
                ));
            } else {
                out.push('[');
                at += 1;
            }
            if out.len() > 256 * 1024 {
                return Err("Промпт после подстановок превышает 256 КБ.".into());
            }
        }
        if out.len() > 256 * 1024 {
            return Err("Промпт после подстановок превышает 256 КБ.".into());
        }
        Ok(out)
    }
    resolve(text, &map, &duplicate, &mut vec![])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_names_and_unicode_are_resolved_without_shell_interpretation() {
        let server = Server {
            id: "test".into(),
            name: "Наш сервер".into(),
            host: "mc.example.org".into(),
            ..Default::default()
        };
        let vars = variables(&[server], &[], &Context::default());
        assert_eq!(
            expand("Адрес: [данные_айпи_(Наш сервер)]", &vars).unwrap(),
            "Адрес: mc.example.org"
        );
        assert!(expand("[данные_айпи_(Неизвестный)]", &vars).is_err());
    }
    #[test]
    fn snippets_can_use_profile_data_and_cycles_are_reported() {
        let s = Server {
            name: "Prod".into(),
            host: "127.0.0.1".into(),
            ..Default::default()
        };
        let snippets = vec![
            Snippet {
                key: "контекст".into(),
                value: "Работай на [данные_айпи_(Prod)]".into(),
            },
            Snippet {
                key: "цикл".into(),
                value: "[цикл]".into(),
            },
        ];
        let vars = variables(&[s], &snippets, &Context::default());
        assert_eq!(expand("[контекст]", &vars).unwrap(), "Работай на 127.0.0.1");
        assert!(expand("[цикл]", &vars).is_err());
    }
    #[test]
    fn duplicate_names_use_distinct_keys_and_normal_markdown_survives() {
        let a = Server {
            id: "aaaaaaaa-1".into(),
            name: "Prod".into(),
            host: "a".into(),
            ..Default::default()
        };
        let mut b = a.clone();
        b.id = "bbbbbbbb-2".into();
        b.host = "b".into();
        let vars = variables(&[a, b], &[], &Context::default());
        assert!(expand("[данные_айпи_(Prod)]", &vars).is_err());
        assert_eq!(
            expand("[данные_айпи_(Prod#bbbbbbbb)] [link](url)", &vars).unwrap(),
            "b [link](url)"
        );
    }
}
