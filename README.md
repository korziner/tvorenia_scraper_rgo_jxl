# tvorenia_scraper_rgo_jxl
Сниматель страницъ съ продолженіемъ работы - scans to JPEG XL

<img width="1244" height="688" alt="image" src="https://github.com/user-attachments/assets/a892afcb-58c6-4447-956d-0700e89828e2" />

<img width="1367" height="852" alt="image" src="https://github.com/user-attachments/assets/08c06a1c-2ecd-4602-bf8b-71c59139645c" />


## Али об учёбе токмо печаль? — нынешнее разумение подхода «Учебники — всё, что вам нужно»

В нынешнем 1826‑м году от Рождества Христова (сиречь 2026‑м по новому летосчислению) надлежит пересмотреть прежние восторги. Статья господина Микрософта «Учебники — всё, что вам нужно» [1] дала начало славной серии Фи, обучив малую модель (1.3B) на синтетических учебниках, порождённых ГПТ‑3.5. Успех был, но последующие испытания ума (smartness test) показали: радость сия не всеобщая, а с великими ограничениями.

### 📚 Что открыли новые изыскания?

#### 1. О качестве и количестве — новая мерная линейка
**«Quality Over Quantity? Redefining Scaling Laws for Smaller LLMs» (arXiv:2410.03083)** [2]  
Господа из Меты и Айовского университета обучили более 200 моделей до 1.5 млрд весов и предложили **новые законы масштабирования**, куда включили *качество* данных. Для малых моделей качество столь же важно, как и количество. Они ввели меру «эффективных обучающих токенов», вычисляемую через:
- **разнообразие** (коэффициент сжатия; разнообразный текст сжать трудно),
- **синтетическую чистоту** (перплексия для учителя; чем ниже, тем «учебнее»).

Тем самым математически объяснили, почему Фи‑1 работала — но не отменили количество.

#### 2. Великий перебор: более тысячи моделей доказали тщету одной синтетики
**«Demystifying Synthetic Data in LLM Pre‑training» (EMNLP 2025, arXiv:2510.01631)** [3]  
Исследователи (Youssef Emad и др.) затратили свыше 100 тысяч часов на вычислительных машинах, обучив более 1000 языковых моделей. Вот что они узрели:
- **Обучение на одной синтетике (учебниках) не быстрее** обучения на природных веб‑текстах.
- **Чистая учебная синтетика даёт заметно бóльшие потери** на многих заданиях, особенно при малых объёмах данных.
- **Наилучшая доля синтетики в смеси — около 30%** (остальное — природные данные).
- **Генераторы синтетики размером более 8B не лучше генераторов 8B.**
- На чистой учебной синтетике наблюдаются признаки **модельного угасания** (model collapse) — вырождения разнообразия ответов.

Вывод: учебники хороши лишь как *добавка* к природным текстам, но не как единственная пища.

#### 3. Парадокс больших учителей: кто сильнее, тот не всегда лучший наставник
**«Stronger Models are Not Always Stronger Teachers for Instruction Tuning» (NAACL 2025, 2025.naacl-long.224)** [4]  
Господа Сюй, Цзян, Ню, Линь, Пувендран открыли **«парадокс больших моделей»** : для настройки по инструкциям более мощная модель-учитель не всегда даёт лучшие данные для малой модели-ученика. Они ввели меру **совместимости** (Compatibility-Adjusted Reward, CAR). Сие напрямую бьёт по основам подхода «Учебники»: ведь Фи‑1 использовала ГПТ‑3.5 как генератора учебников. Может, более слабый, но совместимый учитель дал бы лучший учебник.

#### 4. Учебный план и падение скорости учения — как хорошие уроки приходятся не вовремя
**«How Learning Rate Decay Wastes Your Best Data in Curriculum-Based LLM Pretraining» (arXiv:2511.18903)** [5]  
Господа Ло и Кайжун заметили коварную вещь. Логичное продолжение учебного подхода — обучать модель по **учебному плану** (curriculum): сперва на простых данных, потом на сложных учебниках. Но стандартный **затухающий шаг скорости учения** (learning rate decay) убивает сию затею. Лучшие данные (учебники) подаются, когда модель уже почти не учится (скорость мала). Предложили два исправления: более пологий спад скорости или замену спада усреднением весов (model averaging).

### 🧠 Итог разума (smartness test)

Первоначальное утверждение «Учебники — всё, что вам нужно» ныне следует считать **неполным и отчасти опровергнутым**. Верно, что высокое качество данных — великое благо. Но:

- **Без меры и смеси** (≈30% синтетики + 70% природы) учебники ведут к потерям и вырождению.
- **Законы масштабирования** надо переписать с учётом качества; количество не отменяется.
- **Выбор учителя** для генерации учебников требует совместимости, а не просто мощи.
- **Учебный план** требует пересмотра графика учения (learning rate schedule).

Таким образом, «доверяй, но проверяй» обретает новую глубину: каждую новую порцию учебников надлежит мерить, смешивать и подавать не как единственное блюдо, а как приправу к доброму природному столу.

---

### 📖 Список истинных источников (все ссылки проверены и ведут прямо к сочинениям, кои названы)

| № | Ссылка | Описание |
|---|--------|-----------|
| [1] | [Textbooks Are All You Need (arXiv:2306.11644)](https://arxiv.org/abs/2306.11644) | Исходная работа Microsoft Research (phi‑1) |
| [2] | [Quality Over Quantity? Redefining Scaling Laws for Smaller LLMs (arXiv:2410.03083)](https://arxiv.org/abs/2410.03083) | Новая масштабирующая линейка с качеством данных; обучено 200+ моделей |
| [3] | [Demystifying Synthetic Data in LLM Pre‑training (arXiv:2510.01631)](https://arxiv.org/abs/2510.01631) | Систематическая проверка на 1000+ моделях; оптимальная доля синтетики ≈30% |
| [4] | [Stronger Models are Not Always Stronger Teachers for Instruction Tuning (NAACL 2025, 2025.naacl-long.224)](https://aclanthology.org/2025.naacl-long.224/) | Парадокс больших учителей и мера совместимости CAR |
| [5] | [How Learning Rate Decay Wastes Your Best Data in Curriculum-Based LLM Pretraining (arXiv:2511.18903)](https://arxiv.org/abs/2511.18903) | Учебный план и затухание скорости учения несовместимы; предложены исправления |

Последнее обновление — июнь 1826 года (2026) от Рождества Христова.

```
rgo_jxl  --checkpoint-every 10 --keep-raw  --rgo-handle https://elib.rgo.ru/handle/123456789/211882 --out rgo_dump  --lossless --retry-failed                 
  crawling collection page https://elib.rgo.ru/handle/123456789/211882...
    found item: https://elib.rgo.ru/handle/123456789/169263
  WARN: no diva/safe-view players found on https://elib.rgo.ru/handle/123456789/169263
    found item: https://elib.rgo.ru/handle/123456789/211881
  WARN: no diva/safe-view players found on https://elib.rgo.ru/handle/123456789/211881
    found item: https://elib.rgo.ru/handle/123456789/211961
      safe-view: found 156 pages
    found item: https://elib.rgo.ru/handle/123456789/211950
      safe-view: found 264 pages
    found item: https://elib.rgo.ru/handle/123456789/228818
 ...
    found item: https://elib.rgo.ru/handle/123456789/211964
      safe-view: found 222 pages
  crawling collection page https://elib.rgo.ru/handle/123456789/211882?offset=0...
  crawling collection page https://elib.rgo.ru/handle/123456789/211882?offset=20...
    found item: https://elib.rgo.ru/handle/123456789/227797
      safe-view: found 330 pages
...
      safe-view: found 410 pages
    found item: https://elib.rgo.ru/handle/123456789/236063
      safe-view: found 159 pages
Checkpoint loaded: downloaded=2391, queue=28248, seen=30639, failed=0, out=rgo_dump
 ...
  saved elib.rgo.ru_safe-view_123456789_212006_1_MDAyX1IucGRmLzI0OA  1144x1881  97446B -> 62014B (64%)
  saved elib.rgo.ru_safe-view_123456789_212006_1_MDAyX1IucGRmLzI0OQ  1214x1765  265737B -> 214332B (81%)
  saved elib.rgo.ru_safe-view_123456789_212006_1_MDAyX1IucGRmLzI1MA  1144x1881  176332B -> 135187B (77%)
  saved elib.rgo.ru_safe-view_123456789_212006_1_MDAyX1IucGRmLzI1MQ  1189x1747  259667B -> 208217B (80%)
  saved elib.rgo.ru_safe-view_123456789_212006_1_MDAyX1IucGRmLzI1Mg  1144x1881  180798B -> 138853B (77%)
  saved elib.rgo.ru_safe-view_123456789_212006_1_MDAyX1IucGRmLzI1Mw  1296x1871  337105B -> 281657B (84%)

```

```
Сниматель сканированныхъ книгъ (diva/IIPImage: elib.rgo.ru, prlib.ru) въ JPEG XL

Употребленіе:
  rgo_jxl [OPTIONS]

Параметры и доводы:
Options:
      --item <ITEMS>
          Страница книги на движкѣ diva.js (elib.rgo.ru/prlib.ru). Можно многократно
      --objectdata <OBJECTDATA>
          Прямой URL objectData-JSON (если не хочется парсить страницу книги)
      --iip <IIP>
          Адресъ IIPImage-сервера (…/iipsrv.fcgi) — для --objectdata
      --imagedir <IMAGEDIR>
          Путь imageDir на серверѣ скановъ — для --objectdata
      --page <PAGES>
          Произвольная HTML-страница, изъ коей брать ссылки на изображенія. Многократно
      --url-template <URL_TEMPLATE>
          Шаблонъ URL съ `
          ` для перебора номеровъ страницъ (см. --from/--to)
      --from <FROM>
          Начальный номеръ для --url-template [default: 1]
      --to <TO>
          Конечный номеръ (включительно) для --url-template
      --pad <PAD>
          Сколькими цифрами дополнять 
           нулями (0 — безъ дополненія) [default: 0]
      --url-list <URL_LIST>
          Файлъ со списком ссылокъ на изображенія, по одной на строку
      --rgo-handle <RGO_HANDLES>
          Коллекция RGO (DSpace handle), напр. https://elib.rgo.ru/handle/123456789/211882. Автоматически найдет всѣ книги в ней и поставит их в очередь
      --out <OUT>
          Выходная папка [default: rgo_jxl_dump]
      --delay-ms <DELAY_MS>
          Почтительная задержка между страницами (мсек) [default: 1500]
      --tile-delay-ms <TILE_DELAY_MS>
          Задержка между запросами плитокъ внутри одной страницы (мсек) [default: 150]
      --tile <TILE>
          Размѣръ плитки при сборкѣ полнаго разрѣшенія (≤ ~1700, иначе серверъ уменьшитъ) [default: 1024]
      --no-full-res
          Не собирать полное разрѣшеніе изъ плитокъ, а брать одиночный кадръ `/full/max/` (быстрѣе, но серверъ ограничиваетъ сторону ~1700 px)
      --limit <LIMIT>
          Остановиться послѣ сего числа новыхъ сохраненій
      --retry-failed
          Въ семъ запускѣ вновь пробовать неудавшіяся страницы, а не оставлять ихъ въ спискѣ ошибокъ
      --referer <REFERER>
          HTTP-заголовокъ Referer (нѣкоторые серверы безъ него отдаютъ 403) [default: https://elib.rgo.ru/]
      --distance <DISTANCE>
          Butteraugli-разстояніе для JPEG XL съ потерями (0..15, меньше — лучше). 1.0 — зрительно безъ потерь. 0.0 — математически безъ потерь [default: 1]
      --lossless
          Честный lossless JPEG XL (полная обратимость по пикселямъ). Игнорируетъ --distance
      --effort <EFFORT>
          Скорость/усиліе кодировщика: lightning, thunder, falcon, cheetah, hare, wombat, squirrel, kitten, tortoise. Медленнѣе — меньше размѣръ [default: squirrel]
      --keep-raw
          Сохранять также собранный исходный JPEG въ raw/ (до пересжатія въ jxl)
      --redownload
          Пересохранять, даже если .jxl уже существуетъ
      --checkpoint-every <CHECKPOINT_EVERY>
          Писать state.json каждыхъ N обработанныхъ страницъ [default: 25]
      --min-bytes <MIN_BYTES>
          Минимальный размѣръ загруженнаго файла въ байтахъ (защита отъ заглушекъ) [default: 1024]
      --no-jxl
          Не сохранять JXL (использовать вмѣстѣ съ --keep-raw)
  -h, --help
          Print help
  -V, --version
```
11-12% -max savings - tested under 32-bit and 65-bit termux Android 9-11

```
rgo_jxl --distance 15 --keep-raw  --url-list <(echo https://elib.rgo.ru/handle/123456789/212035) --out rgo_dump --retry-failed  --effort tortoise --checkpoint-every 10             
  crawling collection page https://elib.rgo.ru/handle/123456789/212035...
    found item: https://elib.rgo.ru/handle/123456789/169263
  WARN: no diva/safe-view players found on https://elib.rgo.ru/handle/123456789/169263
    found item: https://elib.rgo.ru/handle/123456789/232811
  WARN: no diva/safe-view players found on https://elib.rgo.ru/handle/123456789/232811
    found item: https://elib.rgo.ru/handle/123456789/212039
  WARN: no diva/safe-view players found on https://elib.rgo.ru/handle/123456789/212039
    found item: https://elib.rgo.ru/handle/123456789/212036
  WARN: no diva/safe-view players found on https://elib.rgo.ru/handle/123456789/212036
    found item: https://elib.rgo.ru/handle/123456789/212038
  WARN: no diva/safe-view players found on https://elib.rgo.ru/handle/123456789/212038
Checkpoint loaded: downloaded=2482, queue=28167, seen=30649, failed=0, out=rgo_dump
  saved elib.rgo.ru_safe-view_123456789_212021_1_MTAwMDA3OTJfS2hyaXN0aWFuaSwgR3JpZ29yaXkgR3JpZ29yYGV2aWNoICgxODYzLSkuIFZzZW9ic2gucGRmLzQ0  1115x1484  303181B -> 38075B (13%)
  saved elib.rgo.ru_safe-view_123456789_212021_1_MTAwMDA3OTJfS2hyaXN0aWFuaSwgR3JpZ29yaXkgR3JpZ29yYGV2aWNoICgxODYzLSkuIFZzZW9ic2gucGRmLzQ1  1115x1484  311273B -> 36805B (12%)
  saved elib.rgo.ru_safe-view_123456789_212021_1_MTAwMDA3OTJfS2hyaXN0aWFuaSwgR3JpZ29yaXkgR3JpZ29yYGV2aWNoICgxODYzLSkuIFZzZW9ic2gucGRmLzQ2  1115x1484  309986B -> 35202B (11%)
  saved elib.rgo.ru_safe-view_123456789_212021_1_MTAwMDA3OTJfS2hyaXN0aWFuaSwgR3JpZ29yaXkgR3JpZ29yYGV2aWNoICgxODYzLSkuIFZzZW9ic2gucGRmLzQ3  1115x1484  329563B -> 39190B (12%)
  saved elib.rgo.ru_safe-view_123456789_212021_1_MTAwMDA3OTJfS2hyaXN0aWFuaSwgR3JpZ29yaXkgR3JpZ29yYGV2aWNoICgxODYzLSkuIFZzZW9ic2gucGRmLzQ4  1115x1484  321758B -> 36055B (11%)
  saved elib.rgo.ru_safe-view_123456789_212021_1_MTAwMDA3OTJfS2hyaXN0aWFuaSwgR3JpZ29yaXkgR3JpZ29yYGV2aWNoICgxODYzLSkuIFZzZW9ic2gucGRmLzQ5  1115x1484  340916B -> 37307B (11%)
```


