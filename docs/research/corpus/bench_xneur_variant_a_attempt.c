/*
 * Вариант А: точный замер через реальный libxneur 0.20.0.
 *
 * Использует ТОЛЬКО официально установленный публичный заголовок
 * /usr/include/xneur/xneur.h (пакет libxneur-dev) плюс три функции,
 * которые экспортированы в libxneur.so.20 (проверено через `nm -D`), но
 * объявлены только во внутренних, не устанавливаемых заголовках xneur
 * (window.h, switchlang.h) -- поэтому их прототипы объявлены здесь вручную:
 *   - struct _window *window_init(struct _xneur_handle *handle);
 *   - int  get_curr_keyboard_group(void);
 *   - void set_keyboard_group(int layout_group);
 *
 * Мы НЕ трогаем внутренние структуры struct _window / struct _buffer --
 * работаем с ними только как с непрозрачными указателями. Это гарантирует
 * ABI-совместимость: единственная структура, чьи поля мы читаем
 * (struct _xneur_handle, struct _xneur_language), взята из официального
 * установленного заголовка, а не из стороннего клона исходников.
 *
 * Что именно тестируем: xneur_get_word(handle, word) -- публичная функция,
 * которая внутри вызывает ТОТ ЖЕ check_lang(), что и живой xneur-демон при
 * наборе текста, с cur_lang = get_curr_keyboard_group().
 *
 * Проблема: cur_lang в реальном xneur -- это АКТИВНАЯ раскладка в момент
 * набора слова. Наш золотой корпус не содержит отдельной колонки с раскладкой
 * набора, только итоговый текст (input). Мы восстанавливаем cur_lang по
 * алфавиту input (кириллица -> 'ru', латиница -> 'us') -- это ровно то же,
 * что представляет собой "раскладка, на которой физически набраны эти
 * символы", и однозначно для всех строк корпуса (см. отчет, ограничения).
 * Чтобы xneur реально использовал нужный cur_lang, мы временно ПЕРЕКЛЮЧАЕМ
 * активную раскладку через set_keyboard_group() (ту же функцию, что дергает
 * хоткей Alt+Shift) перед каждым словом и возвращаем исходную раскладку по
 * завершении. Это обратимое, штатное действие (как обычное переключение
 * раскладки пользователем), не изменяющее конфигурацию системы.
 *
 * Компиляция:
 *   gcc -std=c11 -O2 -o bench_xneur bench_xneur.c $(pkg-config --cflags --libs xneur)
 */

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>

#include <xneur/xneur.h>

/* Функции экспортированы в libxneur.so.20, но объявлены только во
 * внутренних заголовках. Прототипы ниже списаны 1:1 с исходников xneur
 * 0.20 (lib/main/window.h, lib/main/switchlang.h). */
struct _window; /* непрозрачно, полей не касаемся */
extern struct _window *window_init(struct _xneur_handle *handle);
extern int  get_curr_keyboard_group(void);
extern void set_keyboard_group(int layout_group);

/* main_window уже объявлен как extern в xneur.h */

#define MAX_LINE 8192
#define MAX_FIELDS 8

static int split_tsv(char *line, char **fields, int max_fields)
{
    int n = 0;
    char *p = line;
    fields[n++] = p;
    while (*p && n < max_fields)
    {
        if (*p == '\t')
        {
            *p = '\0';
            fields[n++] = p + 1;
        }
        p++;
    }
    return n;
}

static void rstrip(char *s)
{
    size_t len = strlen(s);
    while (len > 0 && (s[len - 1] == '\n' || s[len - 1] == '\r'))
        s[--len] = '\0';
}

/* Восстановление cur_lang по алфавиту байт UTF-8: кириллица -> "ru",
 * латиница -> "us". Смешанное/отсутствие букв -> "us" по умолчанию. */
static int guess_group_index(const unsigned char *s, int ru_group, int us_group)
{
    int has_cyr = 0, has_lat = 0;
    for (const unsigned char *p = s; *p; p++)
    {
        if ((*p >= 'A' && *p <= 'Z') || (*p >= 'a' && *p <= 'z'))
            has_lat = 1;
        if ((p[0] == 0xD0 || p[0] == 0xD1) && p[1] >= 0x80 && p[1] <= 0xBF)
        {
            has_cyr = 1;
            p++; /* пропустить продолжающий байт */
        }
    }
    if (has_cyr && !has_lat)
        return ru_group;
    return us_group; /* us по умолчанию: латиница, смесь, только цифры/символы */
}

static const char *guess_lang_name(const unsigned char *s)
{
    int has_cyr = 0, has_lat = 0;
    for (const unsigned char *p = s; *p; p++)
    {
        if ((*p >= 'A' && *p <= 'Z') || (*p >= 'a' && *p <= 'z'))
            has_lat = 1;
        if ((p[0] == 0xD0 || p[0] == 0xD1) && p[1] >= 0x80 && p[1] <= 0xBF)
        {
            has_cyr = 1;
            p++;
        }
    }
    if (has_cyr && !has_lat)
        return "ru";
    return "us";
}

int main(int argc, char **argv)
{
    if (argc < 3)
    {
        fprintf(stderr, "usage: %s <golden.tsv> <predictions_out.tsv>\n", argv[0]);
        return 2;
    }

    struct _xneur_handle *handle = xneur_handle_create();
    if (handle == NULL)
    {
        fprintf(stderr, "FATAL: xneur_handle_create() returned NULL "
                        "(no X11 display, or XKB rules unavailable)\n");
        return 1;
    }

    fprintf(stderr, "xneur_handle_create: total_languages=%d\n", handle->total_languages);
    int ru_idx = -1, us_idx = -1;
    for (int i = 0; i < handle->total_languages; i++)
    {
        struct _xneur_language *l = &handle->languages[i];
        fprintf(stderr, "  [%d] dir=%s name=%s group=%d excluded=%d disable_auto_detection=%d\n",
                i, l->dir, l->name, l->group, l->excluded, l->disable_auto_detection);
        if (strcmp(l->dir, "ru") == 0) ru_idx = i;
        if (strcmp(l->dir, "us") == 0) us_idx = i;
    }
    if (ru_idx == -1 || us_idx == -1)
    {
        fprintf(stderr, "FATAL: expected both 'ru' and 'us' layouts configured, got ru_idx=%d us_idx=%d\n",
                ru_idx, us_idx);
        xneur_handle_destroy(handle);
        return 1;
    }
    int ru_group = handle->languages[ru_idx].group;
    int us_group = handle->languages[us_idx].group;

    main_window = window_init(handle);
    if (main_window == NULL)
    {
        fprintf(stderr, "FATAL: window_init() returned NULL (no X11 display?)\n");
        xneur_handle_destroy(handle);
        return 1;
    }

    int original_group = get_curr_keyboard_group();
    fprintf(stderr, "original keyboard group = %d (ru_group=%d, us_group=%d)\n",
            original_group, ru_group, us_group);

    FILE *in = fopen(argv[1], "r");
    if (!in)
    {
        perror("fopen input");
        return 1;
    }
    FILE *out = fopen(argv[2], "w");
    if (!out)
    {
        perror("fopen output");
        return 1;
    }

    char line[MAX_LINE];
    char *fields[MAX_FIELDS];

    /* заголовок */
    if (!fgets(line, sizeof(line), in))
    {
        fprintf(stderr, "FATAL: empty input file\n");
        return 1;
    }
    fprintf(out, "id\tcontext\tinput\texpect\tcorrect\tcategory\tguessed_cur_lang\txneur_layout_result\tpredicted_decision\tpredicted_output\n");

    long n_rows = 0;
    while (fgets(line, sizeof(line), in))
    {
        rstrip(line);
        if (line[0] == '\0')
            continue;
        char linecopy[MAX_LINE];
        strncpy(linecopy, line, sizeof(linecopy) - 1);
        linecopy[sizeof(linecopy) - 1] = '\0';

        int nf = split_tsv(linecopy, fields, MAX_FIELDS);
        if (nf < 6)
        {
            fprintf(stderr, "WARN: skipping malformed line (fields=%d): %s\n", nf, line);
            continue;
        }
        const char *id = fields[0];
        const char *context = fields[1];
        const char *input = fields[2];
        const char *expect = fields[3];
        const char *correct = fields[4];
        const char *category = fields[5];

        const char *cur_lang_name = guess_lang_name((const unsigned char *)input);
        int target_group = guess_group_index((const unsigned char *)input, ru_group, us_group);

        set_keyboard_group(target_group);

        char *word_copy = strdup(input);
        int layout_result = xneur_get_layout(handle, word_copy);
        char *new_word = xneur_get_word(handle, word_copy);

        const char *decision;
        const char *output_text;
        if (new_word == NULL)
        {
            decision = "ERROR";
            output_text = input;
        }
        else if (strcmp(new_word, input) == 0)
        {
            decision = "KEEP";
            output_text = new_word;
        }
        else
        {
            decision = "CONVERT";
            output_text = new_word;
        }

        fprintf(out, "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%d\t%s\t%s\n",
                id, context, input, expect, correct, category,
                cur_lang_name, layout_result, decision, output_text);

        free(word_copy);
        if (new_word) free(new_word);
        n_rows++;
        if (n_rows % 200 == 0)
            fprintf(stderr, "... %ld rows processed\n", n_rows);
    }

    fclose(in);
    fclose(out);

    /* восстановить исходную раскладку */
    set_keyboard_group(original_group);

    fprintf(stderr, "Done. %ld rows processed. Restored keyboard group to %d.\n", n_rows, original_group);

    xneur_handle_destroy(handle);
    return 0;
}
