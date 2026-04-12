# 🚀 КОМПЛЕКСНЫЙ ПРОМТ: Telegram Task Manager Bot на Rust

## 📋 ПОЛНОЕ ОПИСАНИЕ ПРОЕКТА

### **Общая суть:**
Разработать **production-ready Telegram бот** для интеллектуального управления задачами сотрудников с AI-поддержкой, историей задач, уведомлениями и отслеживанием статуса.

**Основной workflow:**
1. Пользователь отправляет сообщение (текст или голос) с информацией о задаче
2. Бот парсит сообщение, извлекает данные (исполнитель, описание, срок)
3. Ищет сотрудника в Google Sheets по имени
4. Отправляет в Google Gemini AI для создания структурированного ТЗ по SMART
5. Сохраняет задачу в локальную БД (SQLite)
6. Отправляет ТЗ в Telegram исполнителю и автору
7. Отслеживает статус выполнения и логирует все действия

---

## 🏗️ АРХИТЕКТУРА СИСТЕМЫ (Идеальная версия)

```
┌─────────────────────────────────────────────────────────────┐
│              TELEGRAM TASK MANAGER BOT (Rust)               │
└─────────────────────────────────────────────────────────────┘

LAYER 1: PRESENTATION (Telegram Interface)
├─ Message Receiver (Teloxide)
├─ Command Parser
└─ Response Formatter

LAYER 2: APPLICATION (Business Logic)
├─ Message Parser (Extract task data)
├─ Workflow Orchestrator
├─ Voice Processor (Whisper)
└─ AI Handler (Gemini)

LAYER 3: INTEGRATION (External Services)
├─ Google Sheets Service
├─ Google Gemini Service
├─ OpenAI Whisper Service
└─ Telegram API Service

LAYER 4: DATA PERSISTENCE (Database & Logging)
├─ SQLite Database
├─ Task Repository
├─ User Repository
├─ Audit Log Repository
└─ Logging System (Tracing)

LAYER 5: UTILITIES & HELPERS
├─ Text Parser (Name extraction, deadline parsing)
├─ SMART Validator
├─ Name Matcher (Fuzzy matching)
├─ Date/Time Parser
└─ Error Handler
```

---

## 📊 ДЕТАЛЬНАЯ ЛОГИКА КАЖДОГО КОМПОНЕНТА

### **1. MESSAGE RECEIVER & PARSER**

**Входные данные:**
```rust
// Поддерживаемые типы сообщений:
enum MessageType {
    Text(String),           // Обычное текстовое сообщение
    Voice {
        file_id: String,
        duration: u32,      // Длина аудио в секундах
    },
    Command(String),        // /start, /help, /status и т.д.
}

struct IncomingMessage {
    message_id: i32,
    chat_id: i64,
    sender_id: u32,
    sender_name: String,
    content: MessageType,
    timestamp: DateTime<Utc>,
}
```

**Логика парсинга текста:**
```
Input: "Иван, нужно пофиксить баг с корзиной на сайте. Клиенты жалуются. Срок до завтра."

Шаг 1: EXTRACT ASSIGNEE
├─ Regex: ^([А-Яа-яЁё]+(?:\s+[А-Яа-яЁё]+)?)\s*,\s*
├─ Extract: "Иван"
├─ Generate variations: ["Иван", "Иван Петров", "Иванов" и т.д.]
└─ Status: FOUND

Шаг 2: EXTRACT DEADLINE
├─ Regex patterns:
│  ├─ "до (\d{1,2}\.\d{1,2})" → конкретная дата
│  ├─ "(завтра|сегодня|вчера)" → относительная дата
│  ├─ "через (\d+) (дней|часов)" → период
│  ├─ "(понедельник|вторник|...)" → день недели
│  └─ "срочно" → ASAP (сегодня)
├─ Parse: "до завтра" → завтрашняя дата в формате DD.MM.YYYY
└─ Status: FOUND

Шаг 3: EXTRACT DESCRIPTION
├─ Remove assignee prefix and deadline info
├─ Clean whitespace and normalize
└─ Result: "нужно пофиксить баг с корзиной на сайте. Клиенты жалуются."

Output struct ParsedMessage {
    assignee_name: Some("Иван"),
    task_description: "нужно пофиксить баг...",
    deadline: Some(2026-04-25),
    deadline_raw: "до завтра",
    is_valid: true,
    confidence: 0.95,
}
```

**Validation:**
```
✓ Has assignee name or explicitly "no assignee"
✓ Has task description (min 10 chars)
✓ Has deadline (explicit or "not specified")
✓ Message length reasonable (max 2000 chars)
✓ No spam/gibberish detected
```

---

### **2. VOICE MESSAGE PROCESSING**

**Полный workflow для голосовых сообщений:**

```
INPUT: Voice Message (OGG/MP3/M4A)
    │
    ├─ 2.1 DOWNLOAD & VALIDATE
    │   ├─ Get file_id from Telegram
    │   ├─ Download binary data
    │   ├─ Validate: max 25MB, audio format
    │   ├─ Cache locally for retry mechanism
    │   └─ Status: DOWNLOADED
    │
    ├─ 2.2 TRANSCRIBE (OpenAI Whisper)
    │   ├─ Send to: https://api.openai.com/v1/audio/transcriptions
    │   ├─ Language: "ru" (Russian)
    │   ├─ Timeout: 60 seconds
    │   ├─ Retry on failure: 3 attempts with exponential backoff
    │   ├─ Response: { "text": "Иван, нужно пофиксить баг..." }
    │   └─ Status: TRANSCRIBED
    │
    ├─ 2.3 QUALITY CHECK
    │   ├─ Check if transcription is empty
    │   ├─ Check confidence level (if available)
    │   ├─ If low quality: Ask user to resend
    │   └─ Status: QUALITY_CHECKED
    │
    └─ 2.4 CONTINUE AS TEXT
        └─ Pass to MESSAGE PARSER (как обычный текст)
```

**Error handling for voice:**
```
Scenario 1: File too large (>25MB)
└─ Send: "Голосовое сообщение слишком большое. Макс 25MB"

Scenario 2: Transcription failed
├─ Retry mechanism: 3 попытки
├─ If still fails: Send to manual processing queue
└─ Send: "Не удалось расшифровать. Отправьте текстом"

Scenario 3: Empty transcription
└─ Send: "Аудио слишком короткое или неразборчиво"

Scenario 4: Network timeout
├─ Queue for retry (async)
└─ Send: "Обработка займёт немного времени..."
```

---

### **3. EMPLOYEE LOOKUP (Google Sheets)**

**Интеграция с Google Sheets:**

```
Таблица структура:
┌───┬─────────────────┬──────────────┬────────────────┐
│ID │ Full Name       │ Telegram NN  │ Email          │
├───┼─────────────────┼──────────────┼────────────────┤
│1  │ Иван Петров     │ ivan_petrov  │ ivan@company   │
│2  │ Мария Сидорова  │ maria_side   │ maria@company  │
│3  │ Алексей Морозов │ alex_m       │ alex@company   │
└───┴─────────────────┴──────────────┴────────────────┘

Логика поиска (3-уровневый алгоритм):

SEARCH(assignee_name: "Иван")
│
├─ LEVEL 1: EXACT MATCH
│   └─ "Иван Петров" == "Иван Петров" → Found!
│       Return: Employee {
│           name: "Иван Петров",
│           username: "@ivan_petrov",
│           email: "ivan@company.com"
│       }
│
├─ LEVEL 2: FIRST NAME MATCH + FUZZY
│   ├─ Extract first name: "Иван"
│   ├─ Find all employees with first name "Иван"
│   ├─ Apply Levenshtein distance for matching
│   ├─ If ambiguous: Ask user to clarify
│   └─ If found: Return employee
│
└─ LEVEL 3: PARTIAL MATCH
    ├─ Search across full name using fuzzy matching
    ├─ Threshold: > 80% similarity
    ├─ Rank by relevance
    └─ If not found or low confidence:
        Return: None
        Send to user: "Сотрудник не найден. Укажите полное имя."

Optimization:
├─ Cache employees in memory (reload every 1 hour)
├─ Implement LRU cache for recent searches
└─ Log all searches for analytics
```

**Fallback scenarios:**
```
If employee NOT found:
├─ Create task WITHOUT assignee
├─ Notify task creator: "Сотрудник не найден, задача создана без назначения"
├─ Log warning with search details
└─ Continue workflow

If multiple matches found:
├─ Show inline buttons with candidates
├─ Let user select correct employee
└─ Continue after selection
```

---

### **4. AI TASK CREATION (Google Gemini)**

**Система промптинга:**

```
SYSTEM PROMPT (Основной контекст):
"""
Ты — AI-ассистент в системе управления задачами для Telegram.
Твоя роль: преобразовать неструктурированное описание в четкое техническое задание.

ВАЖНЫЕ ПРАВИЛА:
1. Используй ТОЛЬКО информацию из сообщения пользователя
2. НЕ придумывай детали, которых нет
3. Структурируй по SMART принципам
4. Если информации недостаточно — отмечай как "требует уточнения"
5. Формат ВСЕГДА одинаковый (см. ниже)
6. Язык: РУССКИЙ, профессиональный тон

SMART CRITERIA:
- S (Specific): Конкретно описано, что нужно сделать?
- M (Measurable): Можно ли измерить результат?
- A (Achievable): Реально ли выполнить?
- R (Relevant): Соответствует ли целям организации?
- T (Time-bound): Четкий дедлайн?
"""

PROMPT TEMPLATE (Для каждого сообщения):
"""
Исходные данные:
- Исполнитель: {assignee_name или "Не указан"}
- Сообщение пользователя: {user_message}
- Дата сообщения: {current_date}
- Срок (если указан): {deadline}

Задача: Создай структурированное техническое задание по SMART принципам.

Требуемый ТОЧНЫЙ формат ответа (без отклонений):

[ЕСЛИ ИСПОЛНИТЕЛЬ НАЙДЕН]
@{username}

Заголовок: {task_title_max_100_chars}

Описание (пошагово):
1. {step_1}
2. {step_2}
3. {step_3}
... (добавляй шаги если нужно)

Ожидаемый результат: {measurable_result}

Критерии приёма:
- {acceptance_criteria_1}
- {acceptance_criteria_2}

Срок выполнения: {deadline_formatted_DD.MM.YYYY}

---

[ЕСЛИ ИСПОЛНИТЕЛЬ НЕ НАЙДЕН]
Заголовок: {task_title}

Описание (пошагово):
1. {step_1}
2. {step_2}
... (остальное как выше)
"""

ВАЖНЫЕ ПРИМЕЧАНИЯ:
- Максимум 5-7 шагов в описании
- Все шаги конкретные и действенные
- Результат должен быть измеримым
- Deadline обязательно в формате DD.MM.YYYY
- Если срок не указан: "Срок не указан"
"""

API Call settings:
├─ Model: gemini-2.0-flash (самый быстрый и бесплатный tier)
├─ Max tokens: 1000
├─ Temperature: 0.3 (низкая для консистентности)
├─ Timeout: 15 seconds
└─ Retry: 2 attempts on failure
```

**Response parsing:**

```rust
struct AIGeneratedTask {
    assignee_mention: Option<String>,      // @username или None
    title: String,                          // Заголовок задачи
    steps: Vec<String>,                     // Пошаговое описание
    expected_result: String,                // Ожидаемый результат
    acceptance_criteria: Vec<String>,       // Критерии приёма
    deadline: Option<DateTime<Utc>>,        // Parsed deadline
    deadline_raw: String,                   // Original deadline string
}

// Parsing logic:
// 1. Split response by "---" или section headers
// 2. Extract each field using regex
// 3. Validate structure
// 4. Handle malformed responses with fallback
```

---

### **5. DATABASE SCHEMA & PERSISTENCE**

**SQLite Schema (идеальная структура):**

```sql
-- Users Table
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    telegram_id INTEGER UNIQUE NOT NULL,
    telegram_username TEXT,
    full_name TEXT,
    is_employee BOOLEAN DEFAULT FALSE,
    role TEXT DEFAULT 'user',  -- 'user', 'manager', 'admin'
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_users_telegram_id ON users(telegram_id);

-- Employees Table (из Google Sheets)
CREATE TABLE employees (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    full_name TEXT UNIQUE NOT NULL,
    telegram_username TEXT,
    email TEXT,
    phone TEXT,
    department TEXT,
    is_active BOOLEAN DEFAULT TRUE,
    synced_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_employees_name ON employees(full_name);
CREATE INDEX idx_employees_username ON employees(telegram_username);

-- Tasks Table (основная таблица)
CREATE TABLE tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    
    -- Идентификаторы
    task_uid TEXT UNIQUE NOT NULL,  -- UUID для внешних систем
    
    -- Участники
    created_by_user_id INTEGER NOT NULL,
    assigned_to_user_id INTEGER,  -- NULL если не назначена
    assigned_to_employee_id INTEGER,
    
    -- Содержимое задачи
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    acceptance_criteria TEXT,  -- JSON array
    expected_result TEXT,
    
    -- Сроки
    deadline DATE,
    deadline_raw TEXT,  -- "до завтра", "к 25.04"
    
    -- Источник сообщения
    original_message TEXT NOT NULL,
    message_type TEXT,  -- 'text' или 'voice'
    
    -- AI данные
    ai_model_used TEXT,  -- 'gemini-2.0-flash' и т.д.
    ai_response_raw TEXT,  -- Сохранять для отладки
    
    -- Статус и жизненный цикл
    status TEXT DEFAULT 'created',  -- created, sent, in_progress, completed, cancelled
    priority TEXT DEFAULT 'medium',  -- low, medium, high, urgent
    
    -- Telegram IDs
    telegram_chat_id INTEGER,
    telegram_message_id INTEGER,  -- ID исходного сообщения
    telegram_task_message_id INTEGER,  -- ID отправленного ТЗ
    
    -- Метаданные
    tags TEXT,  -- JSON array
    
    -- Логирование
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    sent_at TIMESTAMP,
    started_at TIMESTAMP,
    completed_at TIMESTAMP,
    cancelled_at TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (created_by_user_id) REFERENCES users(id),
    FOREIGN KEY (assigned_to_user_id) REFERENCES users(id),
    FOREIGN KEY (assigned_to_employee_id) REFERENCES employees(id)
);
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_deadline ON tasks(deadline);
CREATE INDEX idx_tasks_assigned_to ON tasks(assigned_to_user_id);
CREATE INDEX idx_tasks_created_by ON tasks(created_by_user_id);
CREATE INDEX idx_tasks_created_at ON tasks(created_at DESC);

-- Task History / Audit Log
CREATE TABLE task_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL,
    action TEXT NOT NULL,  -- 'created', 'sent', 'assigned', 'status_changed', 'edited', 'deleted'
    old_status TEXT,
    new_status TEXT,
    changed_by_user_id INTEGER,
    metadata TEXT,  -- JSON с деталями изменения
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (task_id) REFERENCES tasks(id),
    FOREIGN KEY (changed_by_user_id) REFERENCES users(id)
);
CREATE INDEX idx_task_history_task_id ON task_history(task_id);
CREATE INDEX idx_task_history_action ON task_history(action);

-- Notifications Log
CREATE TABLE notifications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER,
    recipient_user_id INTEGER NOT NULL,
    notification_type TEXT NOT NULL,  -- 'task_assigned', 'task_updated', 'deadline_reminder', 'task_completed'
    message TEXT NOT NULL,
    telegram_message_id INTEGER,
    is_sent BOOLEAN DEFAULT FALSE,
    is_read BOOLEAN DEFAULT FALSE,
    sent_at TIMESTAMP,
    read_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (task_id) REFERENCES tasks(id),
    FOREIGN KEY (recipient_user_id) REFERENCES users(id)
);
Create INDEX idx_notifications_user ON notifications(recipient_user_id);
CREATE INDEX idx_notifications_sent ON notifications(is_sent);

-- Application Logs (для отладки)
CREATE TABLE app_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    level TEXT NOT NULL,  -- DEBUG, INFO, WARN, ERROR
    module TEXT,  -- Модуль где произошло
    message TEXT NOT NULL,
    context TEXT,  -- JSON с контекстом
    error_trace TEXT,  -- Stacktrace если error
    user_id INTEGER,
    task_id INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_logs_level ON app_logs(level);
CREATE INDEX idx_logs_created_at ON app_logs(created_at DESC);
```

**Repository Pattern (Rust):**

```rust
// Task Repository
pub trait TaskRepository {
    async fn create(&self, task: NewTask) -> Result<Task>;
    async fn get_by_id(&self, id: i32) -> Result<Option<Task>>;
    async fn get_by_uid(&self, uid: &str) -> Result<Option<Task>>;
    async fn update_status(&self, id: i32, status: TaskStatus) -> Result<()>;
    async fn get_all_pending(&self) -> Result<Vec<Task>>;
    async fn get_user_tasks(&self, user_id: i32) -> Result<Vec<Task>>;
    async fn get_assigned_to_employee(&self, emp_id: i32) -> Result<Vec<Task>>;
    async fn get_overdue(&self) -> Result<Vec<Task>>;
    async fn delete(&self, id: i32) -> Result<()>;
}

// Notification Repository
pub trait NotificationRepository {
    async fn create(&self, notif: NewNotification) -> Result<Notification>;
    async fn mark_as_sent(&self, id: i32) -> Result<()>;
    async fn mark_as_read(&self, id: i32) -> Result<()>;
    async fn get_pending(&self) -> Result<Vec<Notification>>;
}

// Audit Log Repository
pub trait AuditLogRepository {
    async fn log(&self, action: AuditAction) -> Result<()>;
    async fn get_task_history(&self, task_id: i32) -> Result<Vec<AuditLog>>;
}
```

---

### **6. NOTIFICATION SYSTEM**

**Уведомления для разных событий:**

```
Тип 1: TASK CREATED NOTIFICATION
├─ Когда: Сразу после создания задачи
├─ Кому: Исполнителю (если найден)
├─ Сообщение:
│  "✅ Вам назначена новая задача:\n\n
│   @creator_name создал(а) для вас задачу
│   \n\nЗаголовок: ...\n\n
│   Срок: DD.MM.YYYY\n\n
│   [Кнопка: Принять] [Кнопка: Отложить] [Кнопка: Вопрос]"
└─ Хранить в БД: notifications table

Тип 2: TASK ASSIGNED (for creator)
├─ Когда: После успешной обработки
├─ Кому: Автору задачи
├─ Сообщение: "Задача отправлена @assignee_name"
└─ Хранить в notifications table

Тип 3: STATUS CHANGE NOTIFICATION
├─ Когда: Когда исполнитель меняет статус
├─ Кому: Автору + Исполнителю
├─ Сообщение: "Статус задачи изменился: pending → in_progress"
└─ Хранить в notifications table

Тип 4: DEADLINE REMINDER
├─ Когда: Automated job (ежедневно в 09:00 UTC)
├─ Условие: deadline - today <= 1 день И status != completed
├─ Кому: Исполнителю
├─ Сообщение: "⏰ Напоминание: Задача '{title}' - срок завтра!"
└─ Хранить в notifications table

Тип 5: OVERDUE NOTIFICATION
├─ Когда: Automated job (ежедневно)
├─ Условие: deadline < today И status != completed
├─ Кому: Исполнителю + Менеджеру
├─ Сообщение: "⚠️ ПРОСРОЧЕННАЯ ЗАДАЧА: '{title}'"
└─ Хранить в notifications table

Тип 6: TASK COMPLETED
├─ Когда: Исполнитель отмечает "выполнено"
├─ Кому: Автору задачи
├─ Сообщение: "✅ Задача '{title}' выполнена @assignee_name"
└─ Хранить в notifications table

Тип 7: COMMENT/QUESTION
├─ Когда: Исполнитель нажимает "Вопрос"
├─ Кому: Автору задачи
├─ Сообщение: "❓ @assignee_name задал вопрос по задаче: {question}"
└─ Открыть диалог для обсуждения
```

**Notification Delivery:**

```rust
// Есть два пути доставки:

// Пусть 1: IMMEDIATE (для критичных)
pub async fn send_notification_immediate(
    &self,
    user_id: i32,
    message: String,
) -> Result<()> {
    // Отправить сразу в Telegram
    let msg = self.telegram_api.send_message(chat_id, message).await?;
    
    // Сохранить в БД
    self.notification_repo.create(Notification {
        user_id,
        message,
        telegram_message_id: msg.message_id,
        is_sent: true,
        sent_at: Some(Utc::now()),
        ..Default::default()
    }).await?;
    
    Ok(())
}

// Путь 2: QUEUED (для менее критичных)
pub async fn queue_notification(
    &self,
    user_id: i32,
    message: String,
    delay_minutes: Option<u32>,
) -> Result<()> {
    // Сохранить в БД как unsent
    self.notification_repo.create(Notification {
        user_id,
        message,
        is_sent: false,
        ..Default::default()
    }).await?;
    
    // Background job будет обрабатывать очередь каждые 30 сек
    Ok(())
}

// Background Job для очереди (рекомендуется Tokio task)
pub async fn notification_queue_processor() {
    loop {
        // Получить все unsent notifications
        let pending = notification_repo.get_pending().await.unwrap_or_default();
        
        for notif in pending {
            match self.telegram_api.send_message(
                get_chat_id(notif.user_id).await,
                &notif.message
            ).await {
                Ok(msg) => {
                    notification_repo.mark_as_sent(notif.id).await.ok();
                }
                Err(e) => {
                    // Log и попробуем позже
                    tracing::error!("Failed to send notification {}: {}", notif.id, e);
                }
            }
        }
        
        // Sleep 30 секунд перед следующей попыткой
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
```

---

### **7. TASK STATUS TRACKING & LIFECYCLE**

**State Machine для задач:**

```
                    ┌─────────────────┐
                    │    CREATED      │ ← Сразу после создания AI
                    │  (Ожидание)     │
                    └────────┬────────┘
                             │ (send_task())
                             ▼
                    ┌─────────────────┐
                    │     SENT        │ ← Отправлено исполнителю
                    │  (В очереди)    │
                    └────────┬────────┘
                             │ (исполнитель нажал "Принять")
                             ▼
                    ┌─────────────────┐
                    │  IN_PROGRESS    │ ← Начал выполнение
                    │ (Выполнение)    │
                    └────────┬────────┘
                    ┌────────┴──────────┐
                    │                   │
        (mark_complete)     (pause)    (cancel)
                    │                   │
                    ▼                   ▼
            ┌──────────────┐    ┌────────────┐
            │  COMPLETED   │    │ ON_HOLD    │
            │ (Готово)     │    │ (Пауза)    │
            └──────────────┘    └──────┬─────┘
                                       │ (resume)
                                       ▼
                              ┌─────────────────┐
                              │  IN_PROGRESS    │
                              └────────┬────────┘
                                       │
                                       ▼
                              ┌─────────────────┐
                              │  COMPLETED      │
                              └─────────────────┘

Дополнительные переходы:
├─ Любой статус → CANCELLED (если отменить)
└─ COMPLETED / CANCELLED → ARCHIVED (через 30 дней)

Временные отметки для каждого статуса:
├─ created_at: Когда создана
├─ sent_at: Когда отправлена
├─ started_at: Когда начала выполняться
├─ completed_at: Когда завершена
├─ cancelled_at: Если отменена
└─ Все сохраняются в БД для аналитики
```

**Команды для управления статусом:**

```
User Interface (Inline buttons in Telegram):

1. После отправки ТЗ:
   [✅ Принять] [⏸️ Отложить] [❓ Вопрос]
   
2. Когда статус IN_PROGRESS:
   [✅ Выполнено] [⏸️ Пауза] [❓ Комментарий]
   
3. После завершения:
   [📝 Переоценить] [❌ Отменить] [📊 Статистика]

Handler для каждой кнопки:
├─ accept_task(task_id) → status = IN_PROGRESS, send notification to creator
├─ defer_task(task_id) → status = ON_HOLD, ask for comment
├─ ask_question(task_id) → open dialog, create comment thread
├─ mark_complete(task_id) → status = COMPLETED, notify creator, log time
├─ pause_task(task_id) → status = ON_HOLD, ask reason
└─ cancel_task(task_id) → status = CANCELLED, ask reason, log
```

---

### **8. LOGGING & AUDIT TRAIL**

**Comprehensive Logging Strategy:**

```rust
// Используем `tracing` crate для структурированного логирования

// Level 1: DEBUG (детальная отладочная информация)
tracing::debug!(
    task_id = %task.id,
    assignee = %assignee.name,
    "Task created with AI response"
);

// Level 2: INFO (основные события)
tracing::info!(
    task_id = %task.id,
    status = "sent",
    "Task sent to user"
);

// Level 3: WARN (предупреждения, требующие внимания)
tracing::warn!(
    employee_name = %name,
    query = %search_query,
    "Employee not found, creating task without assignee"
);

// Level 4: ERROR (критичные ошибки)
tracing::error!(
    task_id = %task.id,
    error = %err,
    "Failed to send notification"
);

// Каждый лог имеет контекст:
let span = tracing::info_span!(
    "process_message",
    user_id = %user_id,
    chat_id = %chat_id,
    message_type = ?msg_type
);

let _guard = span.enter();
// Все логи внутри будут иметь контекст ^

// Все логи также сохраняются в SQLite (app_logs table)
// для долгосрочного анализа
```

**Audit Trail в app_logs:**

```
Логируется каждое значимое событие:

1. User Registration
   └─ level: INFO, module: auth, message: "User registered", context: {user_id, telegram_id}

2. Task Creation
   └─ level: INFO, module: tasks, message: "Task created", context: {task_id, created_by, assigned_to}

3. Task Status Change
   └─ level: INFO, module: tasks, message: "Status changed", context: {task_id, old_status, new_status}

4. Google Sheets Sync
   └─ level: INFO, module: sheets, message: "Synced X employees", context: {count, timestamp}

5. API Errors
   └─ level: ERROR, module: api, message: "Gemini API failed", context: {error, retry_count, trace}

6. Voice Processing
   └─ level: INFO, module: voice, message: "Voice transcribed", context: {duration, confidence, length}

7. Search Operations
   └─ level: DEBUG, module: search, message: "Employee search", context: {query, results, time_ms}

All logs queryable:
├─ By level (DEBUG, INFO, WARN, ERROR)
├─ By module (auth, tasks, api, voice, etc.)
├─ By date range
├─ By user_id or task_id
└─ Export to JSON/CSV
```

---

### **9. COMMAND INTERFACE & USER INTERACTIONS**

**Команды для пользователей:**

```
/start
└─ Инициализирует бота, показывает menu

/help
└─ Показывает справку и правила

/new_task
└─ Интерактивная форма для создания задачи
   ├─ Вводит имя сотрудника
   ├─ Вводит описание
   ├─ Вводит срок
   └─ Автоматически создает ТЗ

/my_tasks
└─ Показывает все мои назначенные задачи (для исполнителя)
   ├─ Статус фильтр
   ├─ Дата сортировка
   └─ Quick actions (accept, complete, etc.)

/created_tasks
└─ Показывает все задачи которые я создал (для менеджера)

/edit_task {task_id}
└─ Редактировать существующую задачу
   ├─ Изменить описание
   ├─ Изменить срок
   ├─ Переназначить
   └─ Добавить комментарий

/cancel_task {task_id}
└─ Отменить задачу

/status {task_id}
└─ Показать текущий статус и историю задачи

/stats
└─ Статистика:
   ├─ Всего задач создано
   ├─ Всего выполнено
   ├─ Среднее время выполнения
   └─ Активные задачи

/settings
└─ Настройки пользователя
   ├─ Уведомления (вкл/выкл)
   ├─ Дневное время нотификаций
   └─ Язык

/admin_sync_employees
└─ Синхронизировать сотрудников из Google Sheets (только для admin)

/admin_logs
└─ Просмотр логов (только для admin)
```

---

### **10. SCHEDULER & BACKGROUND JOBS**

**Tasks that run in background:**

```rust
// Job 1: Deadline Reminders (ежедневно 09:00 UTC)
pub async fn deadline_reminder_job() {
    loop {
        // Проверить, наступило ли 09:00
        if should_run_at_time(09, 0).await {
            let tasks = task_repo.get_all_pending().await?;
            
            for task in tasks {
                let days_until_deadline = 
                    (task.deadline - Utc::now().date_naive()).num_days();
                
                if days_until_deadline == 1 {
                    // Отправить напоминание
                    send_notification_to_assignee(
                        &task,
                        format!("⏰ Напоминание: '{}' - срок завтра!", task.title)
                    ).await.ok();
                }
                
                if days_until_deadline == 0 && task.status != "completed" {
                    // Отправить срочное напоминание
                    send_notification_to_assignee(
                        &task,
                        format!("🚨 СРОЧНО: '{}' - срок СЕГОДНЯ!", task.title)
                    ).await.ok();
                }
            }
        }
        
        // Проверять каждый час
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

// Job 2: Overdue Tasks Check (ежедневно 10:00 UTC)
pub async fn overdue_check_job() {
    loop {
        if should_run_at_time(10, 0).await {
            let overdue = task_repo.get_overdue().await?;
            
            for task in overdue {
                let overdue_days = 
                    (Utc::now().date_naive() - task.deadline).num_days();
                
                if overdue_days % 3 == 0 {  // Напомнить каждые 3 дня
                    send_notification_to_both(
                        &task,
                        format!(
                            "⚠️ Задача '{}' просрочена на {} дней!",
                            task.title, overdue_days
                        )
                    ).await.ok();
                }
            }
            
            // Логировать для аналитики
            tracing::warn!(count = overdue.len(), "Overdue tasks found");
        }
        
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

// Job 3: Google Sheets Sync (каждый час)
pub async fn sync_employees_job() {
    loop {
        match sync_employees_from_sheets().await {
            Ok(count) => {
                tracing::info!(synced_count = count, "Employees synced");
                task_history_repo.log(AuditAction {
                    action: "employees_synced",
                    metadata: json!({"count": count}),
                    ..Default::default()
                }).await.ok();
            }
            Err(e) => {
                tracing::error!("Failed to sync employees: {}", e);
            }
        }
        
        // Синхронизировать каждый час
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

// Job 4: Pending Notifications Queue (каждые 30 сек)
pub async fn notification_queue_processor() {
    // (см. раздел выше)
}

// Job 5: Cleanup Old Logs (еженедельно)
pub async fn cleanup_old_logs_job() {
    loop {
        if should_run_on_day(DayOfWeek::Monday) && should_run_at_time(02, 0).await {
            let cutoff_date = Utc::now() - Duration::days(90);
            
            let deleted = app_logs_repo.delete_before(cutoff_date).await?;
            tracing::info!(deleted_count = deleted, "Old logs cleaned up");
        }
        
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

// Launch all jobs at startup
pub async fn start_background_jobs() {
    tokio::spawn(deadline_reminder_job());
    tokio::spawn(overdue_check_job());
    tokio::spawn(sync_employees_job());
    tokio::spawn(notification_queue_processor());
    tokio::spawn(cleanup_old_logs_job());
}
```

---

### **11. ERROR HANDLING & RESILIENCE**

**Comprehensive Error Strategy:**

```rust
// Custom Error Type (для всего приложения)
#[derive(Debug)]
pub enum AppError {
    // API Errors
    TelegramApiError(String),
    GeminiApiError(String),
    WhisperApiError(String),
    SheetsApiError(String),
    
    // Validation Errors
    InvalidMessage(String),
    EmployeeNotFound(String),
    TaskNotFound(String),
    
    // Database Errors
    DatabaseError(String),
    
    // Processing Errors
    VoiceProcessingError(String),
    TextParsingError(String),
    
    // System Errors
    InternalError(String),
}

impl AppError {
    pub fn is_retryable(&self) -> bool {
        matches!(self,
            AppError::GeminiApiError(_) |
            AppError::WhisperApiError(_) |
            AppError::TelegramApiError(_)
        )
    }
}

// Retry logic with exponential backoff
pub async fn retry_with_backoff<F, T>(
    mut f: impl FnMut() -> F,
    max_attempts: u32,
) -> Result<T, AppError>
where
    F: std::future::Future<Output = Result<T, AppError>>,
{
    let mut attempt = 0;
    
    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) if !e.is_retryable() => return Err(e),
            Err(e) => {
                attempt += 1;
                if attempt >= max_attempts {
                    return Err(e);
                }
                
                let delay = Duration::from_millis(
                    100 * 2_u64.pow(attempt - 1)  // exponential: 100ms, 200ms, 400ms...
                );
                
                tracing::warn!(
                    attempt = attempt,
                    error = ?e,
                    delay_ms = delay.as_millis(),
                    "Retrying operation"
                );
                
                tokio::time::sleep(delay).await;
            }
        }
    }
}

// Usage example
let result = retry_with_backoff(
    || async {
        gemini_service.create_task(&message).await
    },
    max_attempts: 3
).await;

// Graceful degradation
match result {
    Ok(task) => {
        // Process task
    }
    Err(AppError::GeminiApiError(_)) => {
        // Use template-based task creation instead
        let task = create_task_from_template(&message)?;
        send_task_to_user(&task).await?;
    }
    Err(AppError::EmployeeNotFound(_)) => {
        // Create task without assignee
        let task = create_unassigned_task(&message)?;
        send_task_to_user(&task).await?;
    }
    Err(e) => {
        // Log error and notify user
        tracing::error!("Unrecoverable error: {:?}", e);
        send_error_message_to_user(&e).await?;
    }
}
```

---

### **12. TECHNOLOGY STACK (Free/Open Source)**

```
✅ ОБЯЗАТЕЛЬНЫЕ (Core):
├─ Runtime: Tokio 1.40+ (async/await)
├─ Bot Framework: Teloxide 0.15+ (Telegram API)
├─ Database: SQLite + sqlx 0.8+ (async SQL)
├─ Logging: tracing + tracing-subscriber (structured logging)
├─ HTTP Client: reqwest 0.12+ (async HTTP)
└─ Serialization: serde + serde_json (JSON handling)

✅ ИНТЕГРАЦИИ (APIs):
├─ Google Sheets API: google-sheets1-rs (или direct HTTP)
├─ Google Gemini: Direct HTTP API (no official Rust SDK, но это ок)
├─ OpenAI Whisper: Direct HTTP API
└─ Telegram: Teloxide (wrapper around official API)

✅ УТИЛИТЫ (Utilities):
├─ Date/Time: chrono 0.4+ (date manipulation)
├─ UUID: uuid 1.10+ (unique identifiers)
├─ Regex: regex 1.10+ (pattern matching for parsing)
├─ Fuzzy Matching: strsim 0.11+ (name matching algorithm)
├─ Environment: dotenv 0.15+ (configuration)
└─ Error Handling: anyhow 1.0+ (error propagation)

✅ РАЗРАБОТКА (Development):
├─ Testing: tokio-test (async testing)
├─ Mocking: mockall (mock objects)
├─ Code Quality: clippy (linter)
└─ Documentation: cargo doc (auto docs)

ИТОГО: ВСЕ КОМПОНЕНТЫ БЕСПЛАТНЫЕ И OPEN SOURCE! 🎉
```

---

## 🔧 IMPLEMENTATION CHECKLIST

```
PHASE 1: Setup & Infrastructure
  ☐ Create Rust project structure
  ☐ Setup Tokio async runtime
  ☐ Configure SQLite database
  ☐ Setup environment variables (.env)
  ☐ Initialize Teloxide bot
  ☐ Create logging/tracing system
  ☐ Setup basic error handling

PHASE 2: Database & Models
  ☐ Create SQLite schema
  ☐ Implement sqlx queries
  ☐ Create repository pattern
  ☐ Implement all data models
  ☐ Add migrations (sqlx migrate)
  ☐ Test database operations

PHASE 3: Parsing & Processing
  ☐ Implement message parser (extract name, deadline, description)
  ☐ Implement name variation generator
  ☐ Implement deadline parser (handle all date formats)
  ☐ Add validation logic
  ☐ Create unit tests for parser
  ☐ Handle edge cases

PHASE 4: Google Sheets Integration
  ☐ Setup Google Sheets API authentication
  ☐ Implement employee loading
  ☐ Implement fuzzy name matching (Levenshtein distance)
  ☐ Setup caching mechanism
  ☐ Add sync job (refresh employees)
  ☐ Test with real Google Sheets

PHASE 5: AI Integration (Google Gemini)
  ☐ Setup Google Gemini API
  ☐ Create system prompt
  ☐ Implement task generation
  ☐ Parse AI response
  ☐ Handle API errors
  ☐ Add retry mechanism
  ☐ Test with different messages

PHASE 6: Voice Processing
  ☐ Implement voice file download
  ☐ Setup OpenAI Whisper API
  ☐ Implement transcription
  ☐ Add error handling
  ☐ Add retry logic
  ☐ Test with audio files

PHASE 7: Telegram Integration
  ☐ Implement message handlers
  ☐ Implement command handlers (/start, /help, /status, etc.)
  ☐ Create inline buttons for status updates
  ☐ Implement message sending
  ☐ Handle user interactions
  ☐ Test with real Telegram bot

PHASE 8: Task Lifecycle
  ☐ Implement task creation flow
  ☐ Implement status transitions
  ☐ Add inline buttons for user actions
  ☐ Implement accept/defer/complete handlers
  ☐ Track timestamps
  ☐ Create audit logs

PHASE 9: Notification System
  ☐ Implement immediate notifications
  ☐ Implement queued notifications
  ☐ Create notification types
  ☐ Implement notification processor job
  ☐ Test notification delivery
  ☐ Add notification history

PHASE 10: Background Jobs
  ☐ Implement deadline reminder job
  ☐ Implement overdue check job
  ☐ Implement employee sync job
  ☐ Implement notification queue processor
  ☐ Implement log cleanup job
  ☐ Launch all jobs at startup

PHASE 11: Error Handling & Resilience
  ☐ Create custom error types
  ☐ Implement retry logic with backoff
  ☐ Implement graceful degradation
  ☐ Add comprehensive error logging
  ☐ Handle all edge cases
  ☐ Test failure scenarios

PHASE 12: Testing
  ☐ Unit tests for parser
  ☐ Unit tests for name matching
  ☐ Integration tests for API calls
  ☐ Database tests
  ☐ End-to-end workflow tests
  ☐ Load testing

PHASE 13: Documentation & Deployment
  ☐ Add code documentation
  ☐ Create API documentation
  ☐ Write deployment guide
  ☐ Create Docker config
  ☐ Setup monitoring
  ☐ Deploy to production

TOTAL EFFORT: ~3-4 weeks for one experienced Rust developer
```

---

## 📚 PROJECT STRUCTURE

```
telegram-task-bot/
│
├── src/
│   ├── main.rs                          (Entry point)
│   │
│   ├── config.rs                        (Configuration)
│   │   └─ Load from .env, API keys, DB path
│   │
│   ├── error.rs                         (Error types & handling)
│   │   └─ AppError, Result type, error conversion
│   │
│   ├── models/                          (Data models)
│   │   ├── mod.rs
│   │   ├── task.rs                      (Task, TaskStatus)
│   │   ├── user.rs                      (User, Role)
│   │   ├── employee.rs                  (Employee)
│   │   ├── notification.rs              (Notification)
│   │   └── audit.rs                     (AuditLog)
│   │
│   ├── database/                        (Database layer)
│   │   ├── mod.rs
│   │   ├── connection.rs                (DB connection pool)
│   │   └── migrations/                  (SQLx migrations)
│   │       ├── 001_initial_schema.sql
│   │       ├── 002_add_audit_logs.sql
│   │       └─ ...
│   │
│   ├── repositories/                    (Repository pattern)
│   │   ├── mod.rs
│   │   ├── task_repository.rs
│   │   ├── user_repository.rs
│   │   ├── notification_repository.rs
│   │   └─ audit_repository.rs
│   │
│   ├── services/                        (Business logic)
│   │   ├── mod.rs
│   │   ├── telegram_service.rs          (Telegram Bot API)
│   │   ├── gemini_service.rs            (Google Gemini)
│   │   ├── sheets_service.rs            (Google Sheets)
│   │   ├── whisper_service.rs           (OpenAI Whisper)
│   │   ├── task_service.rs              (Task business logic)
│   │   ├── notification_service.rs      (Notification logic)
│   │   └─ user_service.rs
│   │
│   ├── handlers/                        (Message handlers)
│   │   ├── mod.rs
│   │   ├── message_handler.rs           (Text messages)
│   │   ├── voice_handler.rs             (Voice messages)
│   │   ├── command_handler.rs           (Commands: /start, /help, etc.)
│   │   └─ callback_handler.rs           (Inline button callbacks)
│   │
│   ├── processors/                      (Processing logic)
│   │   ├── mod.rs
│   │   ├── message_parser.rs            (Parse message structure)
│   │   ├── deadline_parser.rs           (Parse dates/times)
│   │   ├── name_matcher.rs              (Fuzzy name matching)
│   │   └─ smart_formatter.rs            (Format SMART tasks)
│   │
│   ├── jobs/                            (Background jobs)
│   │   ├── mod.rs
│   │   ├── deadline_reminder.rs
│   │   ├── overdue_check.rs
│   │   ├── sheets_sync.rs
│   │   ├── notification_processor.rs
│   │   └─ log_cleanup.rs
│   │
│   ├── utils/                           (Utilities)
│   │   ├── mod.rs
│   │   ├── logger.rs                    (Tracing setup)
│   │   ├── date_utils.rs
│   │   ├── string_utils.rs
│   │   └─ constants.rs
│   │
│   └── prompts/                         (AI Prompts)
│       ├── mod.rs
│       ├── system_prompt.rs
│       └─ templates.rs
│
├── tests/                               (Integration tests)
│   ├── parser_tests.rs
│   ├── services_tests.rs
│   ├── integration_tests.rs
│   └─ end_to_end_tests.rs
│
├── migrations/                          (SQLx migrations)
│   └── (managed by sqlx migrate)
│
├── .env.example                         (Environment template)
├── .env.local                           (Actual config, not in git)
├── Cargo.toml                           (Dependencies)
├── Cargo.lock                           (Lock file)
├── sqlx-data.json                       (SQLx compile-time checks)
├── README.md                            (Project documentation)
├── DEPLOYMENT.md                        (Deployment guide)
├── ARCHITECTURE.md                      (This document)
├── docker-compose.yml                   (Docker setup)
└── Dockerfile                           (Container image)
```

---

## 🎯 KEY FEATURES SUMMARY

```
✅ Message Processing:
   • Text messages parsing
   • Voice messages transcription (Whisper)
   • Extract: assignee, description, deadline
   • Validate message structure

✅ Employee Management:
   • Sync from Google Sheets (hourly)
   • Fuzzy name matching (Levenshtein)
   • Fallback to unassigned tasks
   • Caching for performance

✅ AI Task Generation:
   • Google Gemini integration
   • Structured SMART format
   • System prompt with rules
   • Error handling & graceful degradation

✅ Task Lifecycle:
   • States: created, sent, in_progress, on_hold, completed, cancelled
   • Status transitions with validation
   • Timestamps for each state
   • Complete audit trail

✅ Notifications:
   • Multiple notification types
   • Immediate & queued delivery
   • Read/unread tracking
   • Notification history

✅ Background Jobs:
   • Deadline reminders (daily)
   • Overdue task checks (daily)
   • Employee sync (hourly)
   • Notification queue (every 30 sec)
   • Log cleanup (weekly)

✅ Logging & Monitoring:
   • Structured logging (tracing)
   • SQLite audit logs
   • Queryable by level/module/date
   • Error tracking with traces

✅ User Interface:
   • /start, /help, /stats, /settings
   • /new_task, /my_tasks, /created_tasks
   • /edit_task, /cancel_task, /status
   • Inline buttons for quick actions
   • Command autocomplete

✅ Database:
   • SQLite with full schema
   • 7 tables: users, employees, tasks, task_history, notifications, app_logs, comments
   • Proper indexing
   • Foreign keys
   • Timestamps on everything

✅ Error Handling:
   • Custom error types
   • Retry logic with exponential backoff
   • Graceful degradation
   • Comprehensive error logging
```

---

## 💡 BEST PRACTICES & PRINCIPLES

```
✅ Code Quality:
   • Use Result<T> for error handling
   • Repository pattern for data access
   • Dependency injection
   • Comprehensive logging
   • Unit + integration tests

✅ Performance:
   • Async/await throughout (Tokio)
   • Connection pooling (sqlx)
   • Caching for frequently accessed data
   • Batch operations where possible
   • Indexed database queries

✅ Reliability:
   • Retry logic with backoff
   • Graceful error handling
   • Fallback strategies
   • Health checks
   • Comprehensive audit logs

✅ Security:
   • Environment variables for secrets
   • Input validation
   • SQL injection prevention (sqlx)
   • Rate limiting (optional)
   • Secure API communication (HTTPS)

✅ Maintainability:
   • Clear separation of concerns
   • Self-documenting code
   • Comprehensive error messages
   • Structured logging
   • Architecture documentation

✅ Scalability:
   • Stateless design (can run multiple instances)
   • Database can be replaced with PostgreSQL if needed
   • Notification queue can handle bursts
   • Background jobs are non-blocking
   • Can handle 1000s of concurrent users
```

---

## 📝 DEPLOYMENT REQUIREMENTS

```
Production Environment:
├─ Server: Linux (Ubuntu 22.04 LTS recommended)
├─ Rust: 1.75+ (stable)
├─ Database: SQLite (or PostgreSQL for larger scale)
├─ Memory: 512MB minimum, 2GB recommended
├─ Disk: 10GB for database + logs
├─ Network: Outbound HTTPS access to:
│  ├─ api.telegram.org (Telegram Bot API)
│  ├─ generativelanguage.googleapis.com (Google Gemini)
│  ├─ api.openai.com (OpenAI Whisper)
│  ├─ sheets.googleapis.com (Google Sheets)
│  └─ docs.google.com (for accessing sheets)
└─ Timezone: Recommend UTC for scheduled jobs

Docker Setup (Recommended):
├─ Create Dockerfile with Rust 1.75
├─ Multi-stage build for smaller image
├─ SQLite mounted as volume
├─ Environment variables via docker-compose
└─ Health check endpoint

Environment Variables:
├─ TELEGRAM_BOT_TOKEN (from BotFather)
├─ GOOGLE_SHEETS_ID (Spreadsheet ID)
├─ GOOGLE_SHEETS_API_KEY (API key)
├─ GOOGLE_OAUTH_CREDENTIALS (JSON path)
├─ GOOGLE_GEMINI_API_KEY (API key)
├─ OPENAI_API_KEY (API key)
├─ DATABASE_URL (sqlite:./data/app.db)
├─ RUST_LOG (debug, info, warn, error)
└─ SERVER_PORT (8080 for health checks)
```

---

## 🚀 READY TO IMPLEMENT!

Этот промт содержит:
✅ Полную архитектуру системы
✅ Детальную логику для каждого компонента
✅ Примеры кода и паттернов
✅ Полную схему БД
✅ Список всех фич
✅ Чек-лист реализации
✅ Требования для деплоя

Все основано на best practices и production-ready подходе.
Используются только бесплатные open-source библиотеки.
Полностью scalable и maintainable архитектура.