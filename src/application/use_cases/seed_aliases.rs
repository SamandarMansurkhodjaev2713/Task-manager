//! Startup use case: seed Russian common-name diminutives into the alias table.
//!
//! This runs once after the first employee sync at startup.  It is fully
//! idempotent — each call skips aliases that already exist in the table
//! (the unique index on `lower(alias)` prevents duplicates).
//!
//! # Seeding algorithm
//!
//! For each `(diminutive, canonical_first_name)` pair in the built-in list:
//! 1. Find employees whose first name (the first whitespace-delimited token of
//!    `full_name`) normalises to `canonical_first_name` (ё→е, lower-case).
//! 2. If **exactly one** employee matches, create an alias row for that pair.
//! 3. If **zero or multiple** employees match, skip — we never guess which
//!    "Саша" to use, and we never create dangling alias rows.
//!
//! The seeded aliases are intentionally NOT created for every possible
//! diminutive of every possible name — only for first names that are
//! unambiguous in the current employee directory at startup time.

use std::sync::Arc;

use chrono::Utc;

use crate::application::ports::repositories::{AliasRepository, EmployeeRepository};
use crate::domain::errors::AppResult;

/// Russian common-name diminutives/short forms → formal first name mappings.
///
/// Each entry is `(alias, canonical_first_name)` where both sides are
/// already in their un-normalised display form; the seeder normalises them
/// before comparison.
const RUSSIAN_DIMINUTIVES: &[(&str, &str)] = &[
    // Мужские
    ("Ваня", "Иван"),
    ("Ванечка", "Иван"),
    ("Ванюша", "Иван"),
    ("Саша", "Александр"),
    ("Шура", "Александр"),
    ("Петя", "Пётр"),
    ("Петруша", "Пётр"),
    ("Коля", "Николай"),
    ("Колька", "Николай"),
    ("Серёжа", "Сергей"),
    ("Серёга", "Сергей"),
    ("Витя", "Виктор"),
    ("Рома", "Роман"),
    ("Дима", "Дмитрий"),
    ("Митя", "Дмитрий"),
    ("Федя", "Фёдор"),
    ("Ваня", "Иван"),
    ("Паша", "Павел"),
    ("Пашка", "Павел"),
    ("Гена", "Геннадий"),
    ("Гоша", "Георгий"),
    ("Жора", "Георгий"),
    ("Гриша", "Григорий"),
    ("Андрюша", "Андрей"),
    ("Андрюха", "Андрей"),
    ("Лёша", "Алексей"),
    ("Лёха", "Алексей"),
    ("Алёша", "Алексей"),
    ("Кирюша", "Кирилл"),
    ("Костя", "Константин"),
    ("Котя", "Константин"),
    ("Стёпа", "Степан"),
    ("Стёпка", "Степан"),
    ("Слава", "Вячеслав"),
    ("Вася", "Василий"),
    ("Васька", "Василий"),
    ("Миша", "Михаил"),
    ("Мишка", "Михаил"),
    ("Лёва", "Лев"),
    ("Лёнька", "Леонид"),
    ("Лёня", "Леонид"),
    ("Женя", "Евгений"),
    ("Тёма", "Артём"),
    ("Влад", "Владислав"),
    ("Денис", "Денис"),
    ("Игорёк", "Игорь"),
    ("Санёк", "Александр"),
    ("Антоха", "Антон"),
    ("Толик", "Анатолий"),
    ("Максик", "Максим"),
    ("Максим", "Максим"),
    // Женские
    ("Саша", "Александра"),
    ("Шура", "Александра"),
    ("Маша", "Мария"),
    ("Машенька", "Мария"),
    ("Катя", "Екатерина"),
    ("Катюша", "Екатерина"),
    ("Оля", "Ольга"),
    ("Оленька", "Ольга"),
    ("Лена", "Елена"),
    ("Леночка", "Елена"),
    ("Аня", "Анна"),
    ("Анюта", "Анна"),
    ("Наташа", "Наталья"),
    ("Наташка", "Наталья"),
    ("Таня", "Татьяна"),
    ("Танюша", "Татьяна"),
    ("Света", "Светлана"),
    ("Светуля", "Светлана"),
    ("Вика", "Виктория"),
    ("Галя", "Галина"),
    ("Нина", "Нина"),
    ("Жека", "Евгения"),
    ("Женя", "Евгения"),
    ("Ира", "Ирина"),
    ("Ирочка", "Ирина"),
    ("Юля", "Юлия"),
    ("Юляша", "Юлия"),
    ("Люба", "Любовь"),
    ("Любаша", "Любовь"),
    ("Вера", "Вера"),
    ("Надя", "Надежда"),
    ("Надюша", "Надежда"),
    ("Ксюша", "Ксения"),
    ("Ксюха", "Ксения"),
    ("Алина", "Алина"),
    ("Лина", "Ангелина"),
    ("Алёна", "Алёна"),
    ("Настя", "Анастасия"),
    ("Настюша", "Анастасия"),
    ("Лиза", "Елизавета"),
    ("Лизочка", "Елизавета"),
    ("Кристина", "Кристина"),
    ("Кристи", "Кристина"),
    ("Дарья", "Дарья"),
    ("Даша", "Дарья"),
    ("Дашенька", "Дарья"),
    ("Соня", "Софья"),
    ("Сонечка", "Софья"),
    ("Полина", "Полина"),
    ("Поля", "Полина"),
    ("Мила", "Людмила"),
    ("Люда", "Людмила"),
    ("Людмилка", "Людмила"),
    ("Тоня", "Антонина"),
    ("Зина", "Зинаида"),
    ("Рита", "Маргарита"),
    ("Маргарита", "Маргарита"),
    ("Вита", "Виталина"),
    ("Диана", "Диана"),
    ("Ника", "Вероника"),
    ("Валя", "Валентина"),
    ("Валюша", "Валентина"),
    ("Алла", "Алла"),
];

pub struct SeedAliasesUseCase {
    employee_repository: Arc<dyn EmployeeRepository>,
    alias_repository: Arc<dyn AliasRepository>,
}

impl SeedAliasesUseCase {
    pub fn new(
        employee_repository: Arc<dyn EmployeeRepository>,
        alias_repository: Arc<dyn AliasRepository>,
    ) -> Self {
        Self {
            employee_repository,
            alias_repository,
        }
    }

    /// Run the seed.  Safe to call on every startup.
    pub async fn execute(&self) -> AppResult<usize> {
        let employees = self.employee_repository.list_active().await?;
        if employees.is_empty() {
            tracing::debug!("seed_aliases: no active employees, skipping");
            return Ok(0);
        }

        let now = Utc::now();
        let mut pairs: Vec<(i64, &str)> = Vec::new();

        for (alias, canonical_first) in RUSSIAN_DIMINUTIVES {
            let normalized_canonical = normalize_name(canonical_first);

            // Count employees whose first name matches the canonical form.
            let matching: Vec<_> = employees
                .iter()
                .filter(|emp| normalize_first_name(&emp.full_name) == normalized_canonical)
                .collect();

            // Only seed when exactly one employee matches — avoids both
            // "no one to point at" and silent wrong-person routing.
            if matching.len() == 1 {
                if let Some(id) = matching[0].id {
                    pairs.push((id, alias));
                }
            }
        }

        // Dedup: if the same (employee_id, alias) pair appears multiple times
        // in RUSSIAN_DIMINUTIVES (e.g. from different spellings that normalise
        // identically), keep only one.
        pairs.sort_unstable();
        pairs.dedup();

        let inserted = self.alias_repository.seed_many(&pairs, now).await?;
        if inserted > 0 {
            tracing::info!(inserted, "seed_aliases: seeded diminutive aliases");
        }
        Ok(inserted)
    }
}

fn normalize_name(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .replace('ё', "е")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_first_name(full_name: &str) -> String {
    let first = full_name.split_whitespace().next().unwrap_or("");
    normalize_name(first)
}
