//! Per-module display names: fastfetch 2.71.0's FFModuleInfo displayName
//! tables (one per module), keyed by the port's registry name. This is the
//! text ffPrintLogoAndKey prints for a row without a custom key (plus
//! " %u" when the module indexes its rows), the display name interpolated
//! into keys like "Battery (model)", the error-row keys, and the
//! {module-name} format argument of custom keys.
//! The language resolves through Instance::display_name (the display
//! surface's keyLanguage, ported from --key-language /
//! display.key.language). All 76 upstream blocks define all 21 languages,
//! so there is no NULL fallback (ffStrbufSetS would clear the key).
//! Regenerate with fixtures/extract_display_names.py +
//! fixtures/gen_display_names_rs.py.

/// The canonical language codes in FFModuleDisplayName field order, as
/// optionParseLanguageString returns them (lowercase, zh_cn/zh_tw).
pub(crate) const LANGUAGES: &[&str] = &[
    "en", "ar", "cs", "de", "es", "fi", "fr", "gl", "he", "id",
    "it", "ja", "ko", "pl", "pt", "ru", "tr", "uk", "vi", "zh_cn", "zh_tw",
];

/// The language index FF_MODULE_GET_DISPLAY_NAME computes as the
/// offsetof(...) delta; unknown codes cannot happen past the parse.
pub(crate) fn language_index(language: Option<&str>) -> usize {
    match language {
        None | Some("en") => 0,
        Some(code) => LANGUAGES.iter().position(|lang| *lang == code).unwrap_or(0),
    }
}

/// The display name, in upstream registration order.
pub(crate) const MODULE_DISPLAY_NAMES: &[(&str, &[&str; 21])] = &[
    ("battery", &[
        "Battery", // en
        "البطارية", // ar
        "Baterie", // cs
        "Akku", // de
        "Batería", // es
        "Akku", // fi
        "Batterie", // fr
        "Batería", // gl
        "סוללה", // he
        "Baterai", // id
        "Batteria", // it
        "バッテリー", // ja
        "배터리", // ko
        "Bateria", // pl
        "Bateria", // pt
        "Батарея", // ru
        "Pil", // tr
        "Батарея", // uk
        "Pin", // vi
        "电池", // zh_CN
        "電池", // zh_TW
    ]),
    ("bios", &[
        "BIOS", // en
        "BIOS", // ar
        "BIOS", // cs
        "BIOS", // de
        "BIOS", // es
        "BIOS", // fi
        "BIOS", // fr
        "BIOS", // gl
        "BIOS", // he
        "BIOS", // id
        "BIOS", // it
        "BIOS", // ja
        "BIOS", // ko
        "BIOS", // pl
        "BIOS", // pt
        "BIOS", // ru
        "BIOS", // tr
        "BIOS", // uk
        "BIOS", // vi
        "BIOS", // zh_CN
        "BIOS", // zh_TW
    ]),
    ("bluetooth", &[
        "Bluetooth", // en
        "بلوتوث", // ar
        "Bluetooth", // cs
        "Bluetooth", // de
        "Bluetooth", // es
        "Bluetooth", // fi
        "Bluetooth", // fr
        "Bluetooth", // gl
        "בלוטות'", // he
        "Bluetooth", // id
        "Bluetooth", // it
        "ブルートゥース", // ja
        "블루투스", // ko
        "Bluetooth", // pl
        "Bluetooth", // pt
        "Bluetooth", // ru
        "Bluetooth", // tr
        "Bluetooth", // uk
        "Bluetooth", // vi
        "蓝牙设备", // zh_CN
        "藍牙裝置", // zh_TW
    ]),
    ("bluetoothradio", &[
        "Bluetooth Radio", // en
        "راديو بلوتوث", // ar
        "Bluetooth rádio", // cs
        "Bluetooth-Adapter", // de
        "Adaptador Bluetooth", // es
        "Bluetooth-adapteri", // fi
        "Adaptateur Bluetooth", // fr
        "Adaptador Bluetooth", // gl
        "רדיו בלוטות'", // he
        "Radio Bluetooth", // id
        "Adattatore Bluetooth", // it
        "ブルートゥースアダプター", // ja
        "블루투스 어댑터", // ko
        "Adapter Bluetooth", // pl
        "Adaptador Bluetooth", // pt
        "Bluetooth-адаптер", // ru
        "Bluetooth Adaptörü", // tr
        "Радіомодуль Bluetooth", // uk
        "Radio Bluetooth", // vi
        "蓝牙适配器", // zh_CN
        "藍牙配接器", // zh_TW
    ]),
    ("board", &[
        "Board", // en
        "اللوحة الأم", // ar
        "Základní deska", // cs
        "Mainboard", // de
        "Placa base", // es
        "Emolevy", // fi
        "Carte mère", // fr
        "Placa base", // gl
        "לוח אם", // he
        "Papan Induk", // id
        "Scheda madre", // it
        "マザーボード", // ja
        "메인보드", // ko
        "Płyta główna", // pl
        "Placa-mãe", // pt
        "Материнская плата", // ru
        "Anakart", // tr
        "Материнська плата", // uk
        "Bo mạch chủ", // vi
        "主板", // zh_CN
        "主機板", // zh_TW
    ]),
    ("bootmgr", &[
        "Boot Manager", // en
        "مدير الإقلاع", // ar
        "Správce spouštění", // cs
        "Boot-Manager", // de
        "Gestor de arranque", // es
        "Käynnistyslataaja", // fi
        "Gestionnaire de démarrage", // fr
        "Xestor de arranque", // gl
        "מנהל אתחול", // he
        "Manajer Boot", // id
        "Gestore di avvio", // it
        "ブートマネージャー", // ja
        "부트 매니저", // ko
        "Menedżer rozruchu", // pl
        "Gerenciador de inicialização", // pt
        "Менеджер загрузки", // ru
        "Önyükleme Yöneticisi", // tr
        "Менеджер завантаження", // uk
        "Trình quản lý khởi động", // vi
        "启动管理器", // zh_CN
        "啟動管理器", // zh_TW
    ]),
    ("break", &[
        "Break", // en
        "فاصل", // ar
        "Konec řádku", // cs
        "Umbruch", // de
        "Salto de línea", // es
        "Rivinvaihto", // fi
        "Saut de ligne", // fr
        "Salto de liña", // gl
        "מעבר שורה", // he
        "Baris Baru", // id
        "Interruzione", // it
        "改行", // ja
        "줄 바꿈", // ko
        "Przerwa", // pl
        "Quebra", // pt
        "Разрыв", // ru
        "Satır Sonu", // tr
        "Розрив рядка", // uk
        "Xuống dòng", // vi
        "换行", // zh_CN
        "換行", // zh_TW
    ]),
    ("brightness", &[
        "Brightness", // en
        "السطوع", // ar
        "Jas", // cs
        "Helligkeit", // de
        "Brillo", // es
        "Kirkkaus", // fi
        "Luminosité", // fr
        "Brillo", // gl
        "בהירות", // he
        "Kecerahan", // id
        "Luminosità", // it
        "明るさ", // ja
        "밝기", // ko
        "Jasność", // pl
        "Brilho", // pt
        "Яркость", // ru
        "Parlaklık", // tr
        "Яскравість", // uk
        "Độ sáng", // vi
        "亮度", // zh_CN
        "亮度", // zh_TW
    ]),
    ("btrfs", &[
        "BTRFS", // en
        "BTRFS", // ar
        "BTRFS", // cs
        "BTRFS", // de
        "BTRFS", // es
        "BTRFS", // fi
        "BTRFS", // fr
        "BTRFS", // gl
        "BTRFS", // he
        "BTRFS", // id
        "BTRFS", // it
        "BTRFS", // ja
        "BTRFS", // ko
        "BTRFS", // pl
        "BTRFS", // pt
        "BTRFS", // ru
        "BTRFS", // tr
        "BTRFS", // uk
        "BTRFS", // vi
        "BTRFS", // zh_CN
        "BTRFS", // zh_TW
    ]),
    ("camera", &[
        "Camera", // en
        "الكاميرا", // ar
        "Kamera", // cs
        "Kamera", // de
        "Cámara", // es
        "Kamera", // fi
        "Caméra", // fr
        "Cámara", // gl
        "מצלמה", // he
        "Kamera", // id
        "Fotocamera", // it
        "カメラ", // ja
        "카메라", // ko
        "Kamera", // pl
        "Câmera", // pt
        "Камера", // ru
        "Kamera", // tr
        "Камера", // uk
        "Máy ảnh", // vi
        "摄像头", // zh_CN
        "攝像頭", // zh_TW
    ]),
    ("chassis", &[
        "Chassis", // en
        "الهيكل", // ar
        "Skříň", // cs
        "Gehäuse", // de
        "Chasis", // es
        "Alusta", // fi
        "Châssis", // fr
        "Chasis", // gl
        "מארז", // he
        "Casing", // id
        "Telaio", // it
        "シャーシ", // ja
        "섀시", // ko
        "Obudowa", // pl
        "Chassi", // pt
        "Корпус", // ru
        "Kasa", // tr
        "Корпус", // uk
        "Vỏ máy", // vi
        "机箱", // zh_CN
        "機箱", // zh_TW
    ]),
    ("codec", &[
        "Codec", // en
        "الترميز", // ar
        "Kodek", // cs
        "Codec", // de
        "Códec", // es
        "Koodekki", // fi
        "Codec", // fr
        "Códec", // gl
        "קודק", // he
        "Kodek", // id
        "Codec", // it
        "コーデック", // ja
        "코덱", // ko
        "Kodek", // pl
        "Codec", // pt
        "Кодек", // ru
        "Kodek", // tr
        "Кодек", // uk
        "Codec", // vi
        "编解码器", // zh_CN
        "編解碼器", // zh_TW
    ]),
    ("colors", &[
        "Colors", // en
        "الألوان", // ar
        "Barvy", // cs
        "Farben", // de
        "Colores", // es
        "Värit", // fi
        "Couleurs", // fr
        "Cores", // gl
        "צבעים", // he
        "Warna", // id
        "Colori", // it
        "色", // ja
        "색상", // ko
        "Kolory", // pl
        "Cores", // pt
        "Цвета", // ru
        "Renkler", // tr
        "Кольори", // uk
        "Màu sắc", // vi
        "颜色", // zh_CN
        "顏色", // zh_TW
    ]),
    ("command", &[
        "Command", // en
        "الأمر", // ar
        "Příkaz", // cs
        "Befehl", // de
        "Comando", // es
        "Komento", // fi
        "Commande", // fr
        "Comando", // gl
        "פקודה", // he
        "Perintah", // id
        "Comando", // it
        "コマンド", // ja
        "명령어", // ko
        "Polecenie", // pl
        "Comando", // pt
        "Команда", // ru
        "Komut", // tr
        "Команда", // uk
        "Lệnh", // vi
        "命令", // zh_CN
        "命令", // zh_TW
    ]),
    ("cpu", &[
        "CPU", // en
        "المعالج", // ar
        "CPU", // cs
        "CPU", // de
        "CPU", // es
        "Suoritin", // fi
        "CPU", // fr
        "CPU", // gl
        "מעבד", // he
        "CPU", // id
        "CPU", // it
        "CPU", // ja
        "CPU", // ko
        "CPU", // pl
        "CPU", // pt
        "Процессор", // ru
        "CPU", // tr
        "CPU", // uk
        "CPU", // vi
        "CPU", // zh_CN
        "CPU", // zh_TW
    ]),
    ("cpucache", &[
        "CPU Cache", // en
        "ذاكرة المعالج المؤقتة", // ar
        "Mezipaměť CPU", // cs
        "CPU-Cache", // de
        "Caché de la CPU", // es
        "Suorittimen Välimuisti", // fi
        "Cache CPU", // fr
        "Caché da CPU", // gl
        "מטמון מעבד", // he
        "Cache CPU", // id
        "Cache CPU", // it
        "CPUキャッシュ", // ja
        "CPU 캐시", // ko
        "Pamięć podręczna CPU", // pl
        "Cache da CPU", // pt
        "Кэш процессора", // ru
        "CPU Önbelleği", // tr
        "Кеш ЦП", // uk
        "Bộ nhớ đệm CPU", // vi
        "CPU缓存", // zh_CN
        "CPU快取記憶體", // zh_TW
    ]),
    ("cpuusage", &[
        "CPU Usage", // en
        "استخدام المعالج", // ar
        "Využití CPU", // cs
        "CPU-Auslastung", // de
        "Uso de la CPU", // es
        "Suorittimen Käyttö", // fi
        "Utilisation du CPU", // fr
        "Uso da CPU", // gl
        "שימוש במעבד", // he
        "Penggunaan CPU", // id
        "Utilizzo della CPU", // it
        "CPU使用率", // ja
        "CPU 사용량", // ko
        "Użycie CPU", // pl
        "Uso da CPU", // pt
        "Использование процессора", // ru
        "CPU Kullanımı", // tr
        "Використання ЦП", // uk
        "Mức sử dụng CPU", // vi
        "CPU使用率", // zh_CN
        "CPU使用率", // zh_TW
    ]),
    ("cursor", &[
        "Cursor", // en
        "المؤشر", // ar
        "Kurzor", // cs
        "Mauszeiger", // de
        "Cursor", // es
        "Kursori", // fi
        "Curseur", // fr
        "Cursor", // gl
        "סמן", // he
        "Kursor", // id
        "Cursore", // it
        "カーソル", // ja
        "커서", // ko
        "Kursor", // pl
        "Cursor", // pt
        "Курсор", // ru
        "İmleç", // tr
        "Курсор", // uk
        "Con trỏ", // vi
        "光标", // zh_CN
        "游標", // zh_TW
    ]),
    ("custom", &[
        "Custom", // en
        "مخصص", // ar
        "Vlastní", // cs
        "Benutzerdefiniert", // de
        "Personalizado", // es
        "Mukautettu", // fi
        "Personnalisé", // fr
        "Personalizado", // gl
        "מותאם אישית", // he
        "Kustom", // id
        "Personalizzato", // it
        "カスタム", // ja
        "사용자 정의", // ko
        "Niestandardowy", // pl
        "Personalizado", // pt
        "Пользовательский", // ru
        "Özel", // tr
        "Власний", // uk
        "Tùy chỉnh", // vi
        "自定义", // zh_CN
        "自訂", // zh_TW
    ]),
    ("datetime", &[
        "Date & Time", // en
        "التاريخ والوقت", // ar
        "Datum a čas", // cs
        "Datum & Uhrzeit", // de
        "Fecha y hora", // es
        "Päivämäärä ja kellonaika", // fi
        "Date et heure", // fr
        "Data e hora", // gl
        "תאריך ושעה", // he
        "Tanggal & Waktu", // id
        "Data e ora", // it
        "日付と時刻", // ja
        "날짜/시간", // ko
        "Data i godzina", // pl
        "Data e hora", // pt
        "Дата и время", // ru
        "Tarih ve Saat", // tr
        "Дата й час", // uk
        "Ngày giờ", // vi
        "日期时间", // zh_CN
        "日期時間", // zh_TW
    ]),
    ("de", &[
        "Desktop Environment", // en
        "بيئة سطح المكتب", // ar
        "Pracovní prostředí", // cs
        "Desktop-Umgebung", // de
        "Entorno de escritorio", // es
        "Työpöytäympäristö", // fi
        "Environnement de Bureau", // fr
        "Contorno do Escritorio", // gl
        "סביבת שולחן עבודה", // he
        "Lingkungan Desktop", // id
        "Ambiente Desktop", // it
        "デスクトップ環境", // ja
        "데스크탑 환경", // ko
        "Środowisko graficzne", // pl
        "Ambiente de desktop", // pt
        "Рабочее окружение", // ru
        "Masaüstü Ortamı", // tr
        "Стільниче середовище", // uk
        "Môi trường desktop", // vi
        "桌面环境", // zh_CN
        "桌面環境", // zh_TW
    ]),
    ("disk", &[
        "Disk", // en
        "القرص", // ar
        "Disk", // cs
        "Festplatte", // de
        "Disco", // es
        "Levy", // fi
        "Disque", // fr
        "Disco", // gl
        "דיסק", // he
        "Disk", // id
        "Disco", // it
        "ディスク", // ja
        "디스크", // ko
        "Dysk", // pl
        "Disco", // pt
        "Диск", // ru
        "Disk", // tr
        "Диск", // uk
        "Ổ đĩa", // vi
        "磁盘分区", // zh_CN
        "硬碟分割", // zh_TW
    ]),
    ("diskio", &[
        "Disk I/O", // en
        "إدخال/إخراج القرص", // ar
        "Diskové I/O", // cs
        "Festplatten I/O", // de
        "E/S del disco", // es
        "Levyn I/O", // fi
        "Entrée/Sortie disque", // fr
        "E/S do disco", // gl
        "קלט/פלט דיסק", // he
        "I/O Disk", // id
        "I/O del disco", // it
        "ディスク I/O", // ja
        "디스크 I/O", // ko
        "Dysk I/O", // pl
        "Disco I/O", // pt
        "Дисковый I/O", // ru
        "Disk G/Ç", // tr
        "Диск В/В", // uk
        "Đĩa I/O", // vi
        "磁盘 I/O", // zh_CN
        "磁碟 I/O", // zh_TW
    ]),
    ("display", &[
        "Display", // en
        "الشاشة", // ar
        "Displej", // cs
        "Anzeige", // de
        "Pantalla", // es
        "Näyttö", // fi
        "Écran", // fr
        "Pantalla", // gl
        "תצוגה", // he
        "Tampilan", // id
        "Display", // it
        "ディスプレイ", // ja
        "디스플레이", // ko
        "Wyświetlacz", // pl
        "Tela", // pt
        "Дисплей", // ru
        "Ekran", // tr
        "Дисплей", // uk
        "Hiển thị", // vi
        "显示器", // zh_CN
        "顯示器", // zh_TW
    ]),
    ("dns", &[
        "DNS", // en
        "DNS", // ar
        "DNS", // cs
        "DNS", // de
        "DNS", // es
        "DNS", // fi
        "DNS", // fr
        "DNS", // gl
        "DNS", // he
        "DNS", // id
        "DNS", // it
        "DNS", // ja
        "DNS", // ko
        "DNS", // pl
        "DNS", // pt
        "DNS", // ru
        "DNS", // tr
        "DNS", // uk
        "DNS", // vi
        "DNS", // zh_CN
        "DNS", // zh_TW
    ]),
    ("editor", &[
        "Editor", // en
        "المحرر", // ar
        "Editor", // cs
        "Editor", // de
        "Editor", // es
        "Editori", // fi
        "Éditeur", // fr
        "Editor", // gl
        "עורך", // he
        "Editor", // id
        "Editor", // it
        "エディター", // ja
        "편집기", // ko
        "Edytor", // pl
        "Editor", // pt
        "Редактор", // ru
        "Düzenleyici", // tr
        "Редактор", // uk
        "Trình soạn thảo", // vi
        "编辑器", // zh_CN
        "編輯器", // zh_TW
    ]),
    ("font", &[
        "Font", // en
        "الخط", // ar
        "Písmo", // cs
        "Schriftart", // de
        "Fuente", // es
        "Fontti", // fi
        "Police", // fr
        "Fonte", // gl
        "גופן", // he
        "Font", // id
        "Carattere", // it
        "フォント", // ja
        "글꼴", // ko
        "Czcionka", // pl
        "Fonte", // pt
        "Шрифт", // ru
        "Yazı Tipi", // tr
        "Шрифт", // uk
        "Phông chữ", // vi
        "字体", // zh_CN
        "字型", // zh_TW
    ]),
    ("gamepad", &[
        "Gamepad", // en
        "يد التحكم", // ar
        "Gamepad", // cs
        "Gamepad", // de
        "Gamepad", // es
        "Peliohjain", // fi
        "Manette", // fr
        "Gamepad", // gl
        "משטח משחק", // he
        "Gamepad", // id
        "Gamepad", // it
        "ゲームパッド", // ja
        "게임패드", // ko
        "Gamepad", // pl
        "Gamepad", // pt
        "Геймпад", // ru
        "Oyun Kumandası", // tr
        "Геймпад", // uk
        "Tay cầm chơi game", // vi
        "游戏手柄", // zh_CN
        "遊戲手把", // zh_TW
    ]),
    ("gpu", &[
        "GPU", // en
        "معالج الرسوميات", // ar
        "GPU", // cs
        "GPU", // de
        "GPU", // es
        "Grafiikkasuoritin", // fi
        "GPU", // fr
        "GPU", // gl
        "מעבד גרפי", // he
        "GPU", // id
        "GPU", // it
        "GPU", // ja
        "GPU", // ko
        "GPU", // pl
        "GPU", // pt
        "Видеокарта", // ru
        "GPU", // tr
        "GPU", // uk
        "GPU", // vi
        "显卡", // zh_CN
        "顯卡", // zh_TW
    ]),
    ("host", &[
        "Host", // en
        "المضيف", // ar
        "Hostitel", // cs
        "Rechner", // de
        "Host", // es
        "Isäntä", // fi
        "Hôte", // fr
        "Host", // gl
        "מארח", // he
        "Host", // id
        "Host", // it
        "ホスト", // ja
        "호스트", // ko
        "Host", // pl
        "Host", // pt
        "Модель", // ru
        "Cihaz", // tr
        "Хост", // uk
        "Máy chủ", // vi
        "主机", // zh_CN
        "主機", // zh_TW
    ]),
    ("icons", &[
        "Icons", // en
        "الأيقونات", // ar
        "Ikony", // cs
        "Symbole", // de
        "Iconos", // es
        "Kuvakkeet", // fi
        "Icônes", // fr
        "Iconas", // gl
        "סמלים", // he
        "Ikon", // id
        "Icone", // it
        "アイコン", // ja
        "아이콘", // ko
        "Ikony", // pl
        "Ícones", // pt
        "Иконки", // ru
        "Simgeler", // tr
        "Піктограми", // uk
        "Biểu tượng", // vi
        "图标", // zh_CN
        "圖示", // zh_TW
    ]),
    ("initsystem", &[
        "Init System", // en
        "نظام الإقلاع", // ar
        "Init systém", // cs
        "Init-System", // de
        "Sistema de inicio", // es
        "Init-järjestelmä", // fi
        "Système d'init", // fr
        "Sistema de inicio", // gl
        "מערכת אתחול", // he
        "Sistem Init", // id
        "Sistema di init", // it
        "initシステム", // ja
        "초기화 시스템", // ko
        "System init", // pl
        "Sistema de inicialização", // pt
        "Система инициализации", // ru
        "Init Sistemi", // tr
        "Init-система", // uk
        "Hệ thống init", // vi
        "Init 系统", // zh_CN
        "Init 系統", // zh_TW
    ]),
    ("kernel", &[
        "Kernel", // en
        "النواة", // ar
        "Jádro", // cs
        "Kernel", // de
        "Kernel", // es
        "Ydin", // fi
        "Kernel", // fr
        "Kernel", // gl
        "ליבה", // he
        "Kernel", // id
        "Kernel", // it
        "カーネル", // ja
        "커널", // ko
        "Jądro", // pl
        "Kernel", // pt
        "Ядро", // ru
        "Çekirdek", // tr
        "Ядро", // uk
        "Nhân", // vi
        "内核", // zh_CN
        "核心", // zh_TW
    ]),
    ("keyboard", &[
        "Keyboard", // en
        "لوحة المفاتيح", // ar
        "Klávesnice", // cs
        "Tastatur", // de
        "Teclado", // es
        "Näppäimistö", // fi
        "Clavier", // fr
        "Teclado", // gl
        "מקלדת", // he
        "Papan Ketik", // id
        "Tastiera", // it
        "キーボード", // ja
        "키보드", // ko
        "Klawiatura", // pl
        "Teclado", // pt
        "Клавиатура", // ru
        "Klavye", // tr
        "Клавіатура", // uk
        "Bàn phím", // vi
        "键盘", // zh_CN
        "鍵盤", // zh_TW
    ]),
    ("lm", &[
        "Login Manager", // en
        "مدير تسجيل الدخول", // ar
        "Správce přihlášení", // cs
        "Anmeldemanager", // de
        "Gestor de inicio de sesión", // es
        "Sisäänkirjautumishallinta", // fi
        "Gestionnaire de connexion", // fr
        "Xestor de inicio de sesión", // gl
        "מנהל התחברות", // he
        "Manajer Login", // id
        "Gestore di accesso", // it
        "ログインマネージャー", // ja
        "로그인 관리자", // ko
        "Menedżer logowania", // pl
        "Gerenciador de login", // pt
        "Менеджер входа", // ru
        "Giriş Yöneticisi", // tr
        "Менеджер входу", // uk
        "Trình quản lý đăng nhập", // vi
        "登录管理器", // zh_CN
        "登入管理器", // zh_TW
    ]),
    ("loadavg", &[
        "Load Average", // en
        "متوسط الحمل", // ar
        "Průměrná zátěž", // cs
        "Systemlast", // de
        "Promedio de carga", // es
        "Kuormituksen Keskiarvo", // fi
        "Charge moyenne", // fr
        "Carga media", // gl
        "עומס ממוצע", // he
        "Beban Rata-rata", // id
        "Carico medio", // it
        "ロードアベレージ", // ja
        "시스템 부하 평균", // ko
        "Średnie obciążenie", // pl
        "Média de carga", // pt
        "Средняя нагрузка", // ru
        "Sistem Yükü", // tr
        "Середнє навантаження", // uk
        "Tải trung bình", // vi
        "系统负载", // zh_CN
        "系統負載", // zh_TW
    ]),
    ("locale", &[
        "Locale", // en
        "اللغة", // ar
        "Národní prostředí", // cs
        "Sprachraum", // de
        "Configuración regional", // es
        "Alueasetus", // fi
        "Paramètres régionaux", // fr
        "Configuración rexional", // gl
        "לוקאל", // he
        "Lokal", // id
        "Impostazioni locali", // it
        "ロケール", // ja
        "로케일", // ko
        "Ustawienia regionalne", // pl
        "Configuração regional", // pt
        "Локаль", // ru
        "Yerel Ayar", // tr
        "Локаль", // uk
        "Bản địa", // vi
        "语言环境", // zh_CN
        "語言環境", // zh_TW
    ]),
    ("localip", &[
        "Local IP", // en
        "عنوان IP المحلي", // ar
        "Místní IP", // cs
        "Lokale IP", // de
        "IP local", // es
        "Paikallinen IP", // fi
        "IP locale", // fr
        "IP local", // gl
        "IP מקומי", // he
        "IP Lokal", // id
        "IP locale", // it
        "ローカル IP", // ja
        "로컬 IP", // ko
        "Lokalny adres IP", // pl
        "IP local", // pt
        "Локальный IP", // ru
        "Yerel IP", // tr
        "Локальна IP", // uk
        "IP cục bộ", // vi
        "本地 IP", // zh_CN
        "區域 IP", // zh_TW
    ]),
    ("logo", &[
        "Logo", // en
        "الشعار", // ar
        "Logo", // cs
        "Logo", // de
        "Logo", // es
        "Logo", // fi
        "Logo", // fr
        "Logo", // gl
        "לוגו", // he
        "Logo", // id
        "Logo", // it
        "ロゴ", // ja
        "로고", // ko
        "Logo", // pl
        "Logo", // pt
        "Логотип", // ru
        "Logo", // tr
        "Логотип", // uk
        "Logo", // vi
        "徽标", // zh_CN
        "標誌", // zh_TW
    ]),
    ("media", &[
        "Media", // en
        "الوسائط", // ar
        "Média", // cs
        "Medien", // de
        "Multimedia", // es
        "Multimedia", // fi
        "Médias", // fr
        "Multimedia", // gl
        "מדיה", // he
        "Media", // id
        "Media", // it
        "メディア", // ja
        "미디어", // ko
        "Media", // pl
        "Mídia", // pt
        "Медиа", // ru
        "Medya", // tr
        "Медіа", // uk
        "Phương tiện", // vi
        "媒体", // zh_CN
        "媒體", // zh_TW
    ]),
    ("memory", &[
        "Memory", // en
        "الذاكرة", // ar
        "Paměť", // cs
        "Speicher", // de
        "Memoria", // es
        "Keskusmuisti", // fi
        "Mémoire", // fr
        "Memoria", // gl
        "זיכרון", // he
        "Memori", // id
        "Memoria", // it
        "メモリ", // ja
        "메모리", // ko
        "Pamięć", // pl
        "Memória", // pt
        "Память", // ru
        "Bellek", // tr
        "Пам'ять", // uk
        "Bộ nhớ", // vi
        "内存", // zh_CN
        "記憶體", // zh_TW
    ]),
    ("monitor", &[
        "Monitor", // en
        "الشاشة", // ar
        "Monitor", // cs
        "Monitor", // de
        "Monitor", // es
        "Monitori", // fi
        "Moniteur", // fr
        "Monitor", // gl
        "צג", // he
        "Monitor", // id
        "Monitor", // it
        "モニター", // ja
        "모니터", // ko
        "Monitor", // pl
        "Monitor", // pt
        "Монитор", // ru
        "Monitör", // tr
        "Монітор", // uk
        "Màn hình", // vi
        "监视器", // zh_CN
        "監視器", // zh_TW
    ]),
    ("mouse", &[
        "Mouse", // en
        "الفأرة", // ar
        "Myš", // cs
        "Maus", // de
        "Ratón", // es
        "Hiiri", // fi
        "Souris", // fr
        "Rato", // gl
        "עכבר", // he
        "Tetikus", // id
        "Mouse", // it
        "マウス", // ja
        "마우스", // ko
        "Mysz", // pl
        "Mouse", // pt
        "Мышь", // ru
        "Fare", // tr
        "Миша", // uk
        "Chuột", // vi
        "鼠标", // zh_CN
        "滑鼠", // zh_TW
    ]),
    ("netio", &[
        "Network I/O", // en
        "إدخال/إخراج الشبكة", // ar
        "Síťové I/O", // cs
        "Netzwerk I/O", // de
        "E/S de la red", // es
        "Verkon I/O", // fi
        "E/S réseau", // fr
        "E/S da rede", // gl
        "קלט/פלט רשת", // he
        "I/O Jaringan", // id
        "I/O di rete", // it
        "ネットワークI/O", // ja
        "네트워크 I/O", // ko
        "I/O sieciowe", // pl
        "E/S de rede", // pt
        "Сетевой I/O", // ru
        "Ağ G/Ç", // tr
        "Мережа В/В", // uk
        "Mạng I/O", // vi
        "网络 I/O", // zh_CN
        "網路 I/O", // zh_TW
    ]),
    ("opencl", &[
        "OpenCL", // en
        "OpenCL", // ar
        "OpenCL", // cs
        "OpenCL", // de
        "OpenCL", // es
        "OpenCL", // fi
        "OpenCL", // fr
        "OpenCL", // gl
        "OpenCL", // he
        "OpenCL", // id
        "OpenCL", // it
        "OpenCL", // ja
        "OpenCL", // ko
        "OpenCL", // pl
        "OpenCL", // pt
        "OpenCL", // ru
        "OpenCL", // tr
        "OpenCL", // uk
        "OpenCL", // vi
        "OpenCL", // zh_CN
        "OpenCL", // zh_TW
    ]),
    ("opengl", &[
        "OpenGL", // en
        "OpenGL", // ar
        "OpenGL", // cs
        "OpenGL", // de
        "OpenGL", // es
        "OpenGL", // fi
        "OpenGL", // fr
        "OpenGL", // gl
        "OpenGL", // he
        "OpenGL", // id
        "OpenGL", // it
        "OpenGL", // ja
        "OpenGL", // ko
        "OpenGL", // pl
        "OpenGL", // pt
        "OpenGL", // ru
        "OpenGL", // tr
        "OpenGL", // uk
        "OpenGL", // vi
        "OpenGL", // zh_CN
        "OpenGL", // zh_TW
    ]),
    ("os", &[
        "OS", // en
        "نظام التشغيل", // ar
        "OS", // cs
        "Betriebssystem", // de
        "Sistema operativo", // es
        "Käyttöjärjestelmä", // fi
        "Système d'exploitation", // fr
        "Sistema operativo", // gl
        "מערכת הפעלה", // he
        "OS", // id
        "Sistema operativo", // it
        "オペレーティングシステム", // ja
        "운영 체제", // ko
        "System operacyjny", // pl
        "Sistema operacional", // pt
        "Операционная система", // ru
        "İşletim Sistemi", // tr
        "ОС", // uk
        "Hệ điều hành", // vi
        "操作系统", // zh_CN
        "操作系統", // zh_TW
    ]),
    ("packages", &[
        "Packages", // en
        "الحزم", // ar
        "Balíčky", // cs
        "Pakete", // de
        "Paquetes", // es
        "Ohjelmistopaketit", // fi
        "Paquets", // fr
        "Paquetes", // gl
        "חבילות", // he
        "Paket", // id
        "Pacchetti", // it
        "パッケージ", // ja
        "패키지", // ko
        "Pakiety", // pl
        "Pacotes", // pt
        "Пакеты", // ru
        "Paketler", // tr
        "Пакети", // uk
        "Gói phần mềm", // vi
        "软件包", // zh_CN
        "軟體包", // zh_TW
    ]),
    ("physicaldisk", &[
        "Physical Disk", // en
        "القرص الفعلي", // ar
        "Fyzický disk", // cs
        "Physische Festplatte", // de
        "Disco físico", // es
        "Fyysinen Levy", // fi
        "Disque physique", // fr
        "Disco físico", // gl
        "דיסק פיזי", // he
        "Disk Fisik", // id
        "Disco fisico", // it
        "物理ディスク", // ja
        "물리 디스크", // ko
        "Dysk fizyczny", // pl
        "Disco físico", // pt
        "Физический диск", // ru
        "Fiziksel Disk", // tr
        "Фізичний диск", // uk
        "Ổ đĩa vật lý", // vi
        "物理磁盘", // zh_CN
        "物理磁碟", // zh_TW
    ]),
    ("physicalmemory", &[
        "Physical Memory", // en
        "الذاكرة الفعلية", // ar
        "Fyzická paměť", // cs
        "Physischer Speicher", // de
        "Memoria física", // es
        "Fyysinen Muisti", // fi
        "Mémoire physique", // fr
        "Memoria física", // gl
        "זיכרון פיזי", // he
        "Memori Fisik", // id
        "Memoria fisica", // it
        "物理メモリ", // ja
        "물리 메모리", // ko
        "Pamięć fizyczna", // pl
        "Memória física", // pt
        "Физическая память", // ru
        "Fiziksel Bellek", // tr
        "Фізична пам'ять", // uk
        "Bộ nhớ vật lý", // vi
        "物理内存", // zh_CN
        "實體記憶體", // zh_TW
    ]),
    ("player", &[
        "Media Player", // en
        "مشغل الوسائط", // ar
        "Přehrávač médií", // cs
        "Medienplayer", // de
        "Reproductor multimedia", // es
        "Mediasoitin", // fi
        "Lecteur multimédia", // fr
        "Reprodutor multimedia", // gl
        "נגן מדיה", // he
        "Pemutar Media", // id
        "Lettore multimediale", // it
        "メディアプレイヤー", // ja
        "미디어 플레이어", // ko
        "Odtwarzacz multimediów", // pl
        "Reprodutor de mídia", // pt
        "Медиаплеер", // ru
        "Medya Oynatıcı", // tr
        "Медіаплеєр", // uk
        "Trình phát phương tiện", // vi
        "媒体播放器", // zh_CN
        "媒體播放器", // zh_TW
    ]),
    ("poweradapter", &[
        "Power Adapter", // en
        "محول الطاقة", // ar
        "Napájecí adaptér", // cs
        "Netzteil", // de
        "Adaptador de corriente", // es
        "Virta-adapteri", // fi
        "Adaptateur secteur", // fr
        "Adaptador de corrente", // gl
        "מתאם מתח", // he
        "Adaptor Daya", // id
        "Alimentatore", // it
        "電源アダプター", // ja
        "전원 어댑터", // ko
        "Zasilacz", // pl
        "Fonte de alimentação", // pt
        "Адаптер питания", // ru
        "Güç Adaptörü", // tr
        "Блок живлення", // uk
        "Bộ chuyển đổi nguồn", // vi
        "电源适配器", // zh_CN
        "電源轉接器", // zh_TW
    ]),
    ("processes", &[
        "Processes", // en
        "العمليات", // ar
        "Procesy", // cs
        "Prozesse", // de
        "Procesos", // es
        "Prosessit", // fi
        "Processus", // fr
        "Procesos", // gl
        "תהליכים", // he
        "Proses", // id
        "Processi", // it
        "プロセス", // ja
        "프로세스", // ko
        "Procesy", // pl
        "Processos", // pt
        "Процессы", // ru
        "Süreçler", // tr
        "Процеси", // uk
        "Tiến trình", // vi
        "进程数", // zh_CN
        "進程數", // zh_TW
    ]),
    ("publicip", &[
        "Public IP", // en
        "عنوان IP العام", // ar
        "Veřejná IP", // cs
        "Öffentliche IP", // de
        "IP pública", // es
        "Julkinen IP", // fi
        "IP publique", // fr
        "IP pública", // gl
        "IP ציבורי", // he
        "IP Publik", // id
        "IP pubblica", // it
        "パブリックIP", // ja
        "공용 IP", // ko
        "Publiczne IP", // pl
        "IP público", // pt
        "Внешний IP", // ru
        "Genel IP", // tr
        "Публічна IP", // uk
        "IP công khai", // vi
        "公网 IP", // zh_CN
        "公網 IP", // zh_TW
    ]),
    ("separator", &[
        "Separator", // en
        "الفاصل", // ar
        "Oddělovač", // cs
        "Trennlinie", // de
        "Separador", // es
        "Erotin", // fi
        "Séparateur", // fr
        "Separador", // gl
        "מפריד", // he
        "Pemisah", // id
        "Separatore", // it
        "セパレーター", // ja
        "구분선", // ko
        "Separator", // pl
        "Separador", // pt
        "Разделитель", // ru
        "Ayraç", // tr
        "Роздільник", // uk
        "Dấu phân cách", // vi
        "分隔线", // zh_CN
        "分隔線", // zh_TW
    ]),
    ("shell", &[
        "Shell", // en
        "الصدفة", // ar
        "Shell", // cs
        "Shell", // de
        "Shell", // es
        "Komentotulkki", // fi
        "Shell", // fr
        "Shell", // gl
        "מעטפת", // he
        "Shell", // id
        "Shell", // it
        "シェル", // ja
        "셸", // ko
        "Powłoka", // pl
        "Shell", // pt
        "Оболочка", // ru
        "Kabuk", // tr
        "Оболонка", // uk
        "Shell", // vi
        "命令行解释器", // zh_CN
        "命令列直譯器", // zh_TW
    ]),
    ("sound", &[
        "Sound", // en
        "الصوت", // ar
        "Zvuk", // cs
        "Sound", // de
        "Sonido", // es
        "Ääni", // fi
        "Son", // fr
        "Son", // gl
        "קול", // he
        "Suara", // id
        "Audio", // it
        "サウンド", // ja
        "사운드", // ko
        "Dźwięk", // pl
        "Som", // pt
        "Звук", // ru
        "Ses", // tr
        "Звук", // uk
        "Âm thanh", // vi
        "声音", // zh_CN
        "聲音", // zh_TW
    ]),
    ("swap", &[
        "Swap", // en
        "الذاكرة التبادلية", // ar
        "Odkládací prostor", // cs
        "Swap", // de
        "Swap", // es
        "Swap", // fi
        "Swap", // fr
        "Swap", // gl
        "זיכרון החלפה", // he
        "Swap", // id
        "Swap", // it
        "スワップ", // ja
        "스왑", // ko
        "Swap", // pl
        "Swap", // pt
        "Подкачка", // ru
        "Takas", // tr
        "Своп", // uk
        "Swap", // vi
        "交换空间", // zh_CN
        "交換空間", // zh_TW
    ]),
    ("terminal", &[
        "Terminal", // en
        "الطرفية", // ar
        "Terminál", // cs
        "Terminal", // de
        "Terminal", // es
        "Terminaali", // fi
        "Terminal", // fr
        "Terminal", // gl
        "טרמינל", // he
        "Terminal", // id
        "Terminale", // it
        "ターミナル", // ja
        "터미널", // ko
        "Terminal", // pl
        "Terminal", // pt
        "Терминал", // ru
        "Terminal", // tr
        "Термінал", // uk
        "Terminal", // vi
        "终端", // zh_CN
        "終端機", // zh_TW
    ]),
    ("terminalfont", &[
        "Terminal Font", // en
        "خط الطرفية", // ar
        "Písmo terminálu", // cs
        "Terminalschriftart", // de
        "Fuente del terminal", // es
        "Terminaalin Fontti", // fi
        "Police du terminal", // fr
        "Fonte do terminal", // gl
        "גופן טרמינל", // he
        "Font Terminal", // id
        "Carattere del terminale", // it
        "ターミナルフォント", // ja
        "터미널 글꼴", // ko
        "Czcionka terminala", // pl
        "Fonte do terminal", // pt
        "Шрифт терминала", // ru
        "Terminal Yazı Tipi", // tr
        "Шрифт терміналу", // uk
        "Phông chữ Terminal", // vi
        "终端字体", // zh_CN
        "終端機字型", // zh_TW
    ]),
    ("terminalsize", &[
        "Terminal Size", // en
        "حجم الطرفية", // ar
        "Velikost terminálu", // cs
        "Terminalgröße", // de
        "Tamaño del terminal", // es
        "Terminaalin Koko", // fi
        "Taille du terminal", // fr
        "Tamaño do terminal", // gl
        "גודל טרמינל", // he
        "Ukuran Terminal", // id
        "Dimensioni terminale", // it
        "ターミナルサイズ", // ja
        "터미널 크기", // ko
        "Rozmiar terminala", // pl
        "Tamanho do terminal", // pt
        "Размер терминала", // ru
        "Terminal Boyutu", // tr
        "Розмір терміналу", // uk
        "Kích thước Terminal", // vi
        "终端大小", // zh_CN
        "終端機大小", // zh_TW
    ]),
    ("terminaltheme", &[
        "Terminal Theme", // en
        "سمة الطرفية", // ar
        "Motiv terminálu", // cs
        "Terminalthema", // de
        "Tema del terminal", // es
        "Terminaalin Teema", // fi
        "Thème du terminal", // fr
        "Tema do terminal", // gl
        "ערכת נושא של טרמינל", // he
        "Tema Terminal", // id
        "Tema del terminale", // it
        "ターミナルテーマ", // ja
        "터미널 테마", // ko
        "Motyw terminala", // pl
        "Tema do terminal", // pt
        "Тема терминала", // ru
        "Terminal Teması", // tr
        "Тема терміналу", // uk
        "Chủ đề Terminal", // vi
        "终端主题", // zh_CN
        "終端機主題", // zh_TW
    ]),
    ("theme", &[
        "Theme", // en
        "السمة", // ar
        "Motiv", // cs
        "Thema", // de
        "Tema", // es
        "Teema", // fi
        "Thème", // fr
        "Tema", // gl
        "ערכת נושא", // he
        "Tema", // id
        "Tema", // it
        "テーマ", // ja
        "테마", // ko
        "Motyw", // pl
        "Tema", // pt
        "Тема", // ru
        "Tema", // tr
        "Тема", // uk
        "Chủ đề", // vi
        "主题", // zh_CN
        "主題", // zh_TW
    ]),
    ("title", &[
        "Title", // en
        "العنوان", // ar
        "Titulek", // cs
        "Titel", // de
        "Título", // es
        "Otsikko", // fi
        "Titre", // fr
        "Título", // gl
        "כותרת", // he
        "Judul", // id
        "Titolo", // it
        "タイトル", // ja
        "제목", // ko
        "Tytuł", // pl
        "Título", // pt
        "Заголовок", // ru
        "Başlık", // tr
        "Заголовок", // uk
        "Tiêu đề", // vi
        "标题", // zh_CN
        "標題", // zh_TW
    ]),
    ("top", &[
        "Top Processes", // en
        "أعلى العمليات", // ar
        "Nejvytíženější procesy", // cs
        "Top-Prozesse", // de
        "Procesos principales", // es
        "Raskaimmat Prosessit", // fi
        "Processus principaux", // fr
        "Procesos principais", // gl
        "התהליכים המובילים", // he
        "Proses teratas", // id
        "Processi principali", // it
        "上位プロセス", // ja
        "상위 프로세스", // ko
        "Najaktywniejsze procesy", // pl
        "Processos principais", // pt
        "Топ процессов", // ru
        "En Yoğun Süreçler", // tr
        "Найактивніші процеси", // uk
        "Tiến trình hàng đầu", // vi
        "进程排行", // zh_CN
        "進程排行", // zh_TW
    ]),
    ("tpm", &[
        "TPM", // en
        "TPM", // ar
        "TPM", // cs
        "TPM", // de
        "TPM", // es
        "TPM", // fi
        "TPM", // fr
        "TPM", // gl
        "TPM", // he
        "TPM", // id
        "TPM", // it
        "TPM", // ja
        "TPM", // ko
        "TPM", // pl
        "TPM", // pt
        "TPM", // ru
        "TPM", // tr
        "TPM", // uk
        "TPM", // vi
        "TPM", // zh_CN
        "TPM", // zh_TW
    ]),
    ("uptime", &[
        "Uptime", // en
        "مدة التشغيل", // ar
        "Doba běhu", // cs
        "Betriebszeit", // de
        "Tiempo de actividad", // es
        "Käynnissäoloaika", // fi
        "Temps de fonctionnement", // fr
        "Tempo de actividade", // gl
        "זמן פעילות", // he
        "Waktu Aktif", // id
        "Tempo di attività", // it
        "稼働時間", // ja
        "업타임", // ko
        "Czas pracy", // pl
        "Tempo de atividade", // pt
        "Время работы", // ru
        "Çalışma Süresi", // tr
        "Час роботи", // uk
        "Thời gian hoạt động", // vi
        "运行时间", // zh_CN
        "運行時間", // zh_TW
    ]),
    ("users", &[
        "Users", // en
        "المستخدمون", // ar
        "Uživatelé", // cs
        "Benutzer", // de
        "Usuarios", // es
        "Käyttäjät", // fi
        "Utilisateurs", // fr
        "Usuarios", // gl
        "משתמשים", // he
        "Pengguna", // id
        "Utenti", // it
        "ユーザー", // ja
        "사용자", // ko
        "Użytkownicy", // pl
        "Usuários", // pt
        "Пользователи", // ru
        "Kullanıcılar", // tr
        "Користувачі", // uk
        "Người dùng", // vi
        "用户", // zh_CN
        "使用者", // zh_TW
    ]),
    ("version", &[
        "Version", // en
        "الإصدار", // ar
        "Verze", // cs
        "Version", // de
        "Versión", // es
        "Versio", // fi
        "Version", // fr
        "Versión", // gl
        "גרסה", // he
        "Versi", // id
        "Versione", // it
        "バージョン", // ja
        "버전", // ko
        "Wersja", // pl
        "Versão", // pt
        "Версия", // ru
        "Sürüm", // tr
        "Версія", // uk
        "Phiên bản", // vi
        "版本", // zh_CN
        "版本", // zh_TW
    ]),
    ("vulkan", &[
        "Vulkan", // en
        "Vulkan", // ar
        "Vulkan", // cs
        "Vulkan", // de
        "Vulkan", // es
        "Vulkan", // fi
        "Vulkan", // fr
        "Vulkan", // gl
        "Vulkan", // he
        "Vulkan", // id
        "Vulkan", // it
        "Vulkan", // ja
        "Vulkan", // ko
        "Vulkan", // pl
        "Vulkan", // pt
        "Vulkan", // ru
        "Vulkan", // tr
        "Vulkan", // uk
        "Vulkan", // vi
        "Vulkan", // zh_CN
        "Vulkan", // zh_TW
    ]),
    ("wallpaper", &[
        "Wallpaper", // en
        "الخلفية", // ar
        "Tapeta", // cs
        "Hintergrundbild", // de
        "Fondo de pantalla", // es
        "Taustakuva", // fi
        "Fond d'écran", // fr
        "Fondo de escritorio", // gl
        "טפט", // he
        "Wallpaper", // id
        "Sfondo", // it
        "壁紙", // ja
        "배경화면", // ko
        "Tapeta", // pl
        "Papel de parede", // pt
        "Обои", // ru
        "Duvar Kağıdı", // tr
        "Шпалери", // uk
        "Hình nền", // vi
        "壁纸", // zh_CN
        "桌布", // zh_TW
    ]),
    ("weather", &[
        "Weather", // en
        "الطقس", // ar
        "Počasí", // cs
        "Wetter", // de
        "Tiempo", // es
        "Sää", // fi
        "Météo", // fr
        "Tempo", // gl
        "מזג אוויר", // he
        "Cuaca", // id
        "Meteo", // it
        "天気", // ja
        "날씨", // ko
        "Pogoda", // pl
        "Clima", // pt
        "Погода", // ru
        "Hava Durumu", // tr
        "Погода", // uk
        "Thời tiết", // vi
        "天气", // zh_CN
        "天氣", // zh_TW
    ]),
    ("wifi", &[
        "Wi-Fi", // en
        "واي فاي", // ar
        "Wi-Fi", // cs
        "WLAN", // de
        "Wi-Fi", // es
        "Wi-Fi", // fi
        "Wi-Fi", // fr
        "Wi-Fi", // gl
        "Wi-Fi", // he
        "Wi-Fi", // id
        "Wi-Fi", // it
        "Wi-Fi", // ja
        "와이파이", // ko
        "Wi-Fi", // pl
        "Wi-Fi", // pt
        "Wi-Fi", // ru
        "Wi-Fi", // tr
        "Wi-Fi", // uk
        "Wi-Fi", // vi
        "无线网络", // zh_CN
        "無線網路", // zh_TW
    ]),
    ("wm", &[
        "Window Manager", // en
        "مدير النوافذ", // ar
        "Správce oken", // cs
        "Fenstermanager", // de
        "Gestor de ventanas", // es
        "Ikkunointijärjestelmä", // fi
        "Gestionnaire de fenêtres", // fr
        "Xestor de xanelas", // gl
        "מנהל חלונות", // he
        "Manajer Jendela", // id
        "Gestore finestre", // it
        "ウィンドウマネージャー", // ja
        "윈도우 관리자", // ko
        "Menedżer okien", // pl
        "Gerenciador de janelas", // pt
        "Менеджер окон", // ru
        "Pencere Yöneticisi", // tr
        "Менеджер вікон", // uk
        "Trình quản lý cửa sổ", // vi
        "窗口管理器", // zh_CN
        "視窗管理員", // zh_TW
    ]),
    ("wmtheme", &[
        "WM Theme", // en
        "سمة مدير النوافذ", // ar
        "Motiv správce oken", // cs
        "Fenstermanager-Thema", // de
        "Tema del gestor de ventanas", // es
        "Ikkunointijärjestelmän Teema", // fi
        "Thème du gestionnaire de fenêtres", // fr
        "Tema do xestor de xanelas", // gl
        "ערכת נושא של מנהל חלונות", // he
        "Tema Manajer Jendela", // id
        "Tema del gestore finestre", // it
        "ウィンドウマネージャのテーマ", // ja
        "윈도우 관리자 테마", // ko
        "Motyw menedżera okien", // pl
        "Tema do gerenciador de janelas", // pt
        "Тема менеджера окон", // ru
        "WM Teması", // tr
        "Тема менеджера вікон", // uk
        "Chủ đề trình quản lý cửa sổ", // vi
        "窗口管理器主题", // zh_CN
        "視窗管理員主題", // zh_TW
    ]),
    ("zpool", &[
        "Zpool", // en
        "Zpool", // ar
        "Zpool", // cs
        "Zpool", // de
        "Zpool", // es
        "Zpool", // fi
        "Zpool", // fr
        "Zpool", // gl
        "Zpool", // he
        "Zpool", // id
        "Zpool", // it
        "Zpool", // ja
        "Zpool", // ko
        "Zpool", // pl
        "Zpool", // pt
        "Zpool", // ru
        "Zpool", // tr
        "Zpool", // uk
        "Zpool", // vi
        "Zpool", // zh_CN
        "Zpool", // zh_TW
    ]),
];

pub(crate) fn display_name_for(language: Option<&str>, name: &str) -> &'static str {
    let index = language_index(language);
    MODULE_DISPLAY_NAMES
        .iter()
        .find(|(module, _)| *module == name)
        .map_or("", |(_, names)| names[index])
}
