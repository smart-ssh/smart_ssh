# i18n glossary

Rules for all UI translations (Spec 0024). Translation issues fill in the
per-language table.

Project and domain terms are in [`glossary.md`](glossary.md).

## Stay untranslated

SSH, SFTP, sudo, MCP, Smart SSH, Ollama, and other product, protocol and
command names (shell commands, flags, paths, error codes).

## Address

Use formal address where the language distinguishes it (Sie / vous /
usted / …).

## Preferred translations

Filled per language by the translation issues. English is the reference.

| English term | de | fr | es | it | pl | cs | uk | ru | nl | pt-BR | ja | zh-Hans | ko | sv | da | nb | fi | id | vi | hi | tr | el | hu | ro |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---| --- | --- | --- | --- |---|---|---|---|---|---|---|
| filter rule | Filterregel (Regel) | règle de filtrage | regla de filtro | regola di filtro | reguła filtrowania | pravidlo filtru | правило фільтра | правило фильтра | filterregel | regra de filtro | フィルタールール | 过滤规则 | 필터 규칙 | filterregel | filterregel | filterregel | suodatussääntö | aturan filter | quy tắc lọc | फ़िल्टर नियम | filtre kuralı | κανόνας φίλτρου | szűrőszabály | regulă de filtrare |
| allow / deny | Allow / Deny (Ablehnen) | autoriser / refuser | Permitir / Denegar | Consenti / Nega | Zezwól / Odmów | Povolit / Zamítnout | Дозволити / Заборонити | Разрешить / Запретить | toestaan / weigeren | permitir / negar | 許可 / 拒否 | 允许 / 拒绝 | 허용 / 거부 | tillåt / neka | tillad / forbyd | tillat / nekt | salli / estä | Izinkan / Tolak | Cho phép / Từ chối | अनुमति दें / अस्वीकार करें | izin ver / reddet | να επιτρέπεται / απόρριψη | engedélyezés / tiltás | permite / refuză |
| session | Sitzung | session | sesión | sessione | sesja | relace | сеанс | сеанс | sessie | sessão | セッション | 会话 | 세션 | session | session | økt | istunto | sesi | phiên | सत्र | oturum | συνεδρία | munkamenet | sesiune |
| server | Server | serveur | servidor | server | serwer | server | сервер | сервер | server | servidor | サーバー | 服务器 | 서버 | server | server | server | palvelin | server | máy chủ | सर्वर | sunucu | διακομιστής | szerver | server |
| file browser | Dateibrowser | explorateur de fichiers | explorador de archivos | file browser | przeglądarka plików | prohlížeč souborů | провідник файлів | обозреватель файлов | bestandsbeheer | navegador de arquivos | ファイルブラウザー | 文件浏览器 | 파일 브라우저 | filbläddrare | filbrowser | filutforsker | tiedostoselain | penjelajah berkas | trình duyệt tệp | फ़ाइल ब्राउज़र | dosya tarayıcısı | πρόγραμμα περιήγησης αρχείων | fájlböngésző | browser de fișiere |
| redaction | redigiert | caviardage | ocultación de datos sensibles | oscuramento | redakcja (maskowanie danych wrażliwych) | redakce (maskování citlivých údajů) | редагування (приховування чутливих даних) | редактирование (скрытие конфиденциальных данных) | maskering | ocultação de dados sensíveis | マスキング（機密データの伏せ字化） | 脱敏（隐藏敏感数据） | 마스킹(민감 데이터 가리기) | maskning | maskering | sladding | häivytys | penyamaran data sensitif | che giấu dữ liệu nhạy cảm | संवेदनशील डेटा छिपाना | maskeleme (hassas verilerin maskelenmesi) | απόκρυψη ευαίσθητων δεδομένων | kitakarás (érzékeny adatok kitakarása) | mascare (mascarea datelor sensibile) |
| second opinion | Zweitmeinung | second avis | segunda opinión | seconda opinione | druga opinia | druhý názor | друга думка | второе мнение | second opinion | segunda opinião | セカンドオピニオン | 二次意见 | 세컨드 오피니언 | andra åsikt | anden vurdering | andremening | toinen mielipide | opini kedua | ý kiến thứ hai | दूसरी राय | ikinci görüş | δεύτερη γνώμη | második vélemény | a doua opinie |

The `sv`, `da`, `nb` and `fi` columns were filled with the Nordic languages
(Issue #333); `tr`, `el`, `hu` and `ro` were added with Issue #336. All are machine translations without human review. 

Note: Norwegian (`nb`) uses the neutral/imperative form and avoids "du"
where possible; Swedish, Danish and Finnish likewise prefer neutral wording
over a personal pronoun.
