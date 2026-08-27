# #208 — Converter os 58 timestamps de texto para `timestamptz`

## Porquê

As colunas de tempo do Sentinel são `text` com RFC3339 porque o `sqlx` era compilado sem a feature
`chrono` e não sabia descodificar `timestamptz`. **Essa feature está agora ligada** (aditiva, os 272
testes passam), portanto o motivo desapareceu — mas as 58 colunas continuam texto.

O que isso custa hoje, e não é estético:

- **Comparações são lexicográficas.** `where created_at > $1` funciona por acidente: o RFC3339 ordena
  como texto *se* o fuso for sempre o mesmo e os dígitos estiverem sempre preenchidos. Uma linha
  escrita com `-03:00` em vez de `Z` ordena errado e ninguém repara.
- **Não há aritmética de datas.** `now() - interval '7 days'` obriga a um cast em cada consulta, e
  `date_trunc` para agrupar por dia é um cast mais um parse.
- **Nenhum índice serve para intervalos** sem cast, e um cast na cláusula `where` deita fora o índice.
- **101 chamadas a `now_rfc3339()`** que passariam a ser `now()` no SQL.

## O que muda

Uma migração por tabela, e cada coluna é:

```sql
ALTER TABLE <t> ALTER COLUMN <c> TYPE timestamptz USING <c>::timestamptz;
```

O RFC3339 converte limpo — **nenhuma linha se perde e nenhum grafo é tocado**. Os grafos vivem no
`data jsonb`; os timestamps são colunas ao lado.

Depois: os `Option<String>` viram `Option<DateTime<Utc>>` nos structs, os `::text` saem das queries,
e as `now_rfc3339()` viram `now()`.

## O que pode correr mal, e a resposta

**Uma linha com formato inesperado** derruba o `ALTER` inteiro — ele é uma transacção. Antes de
migrar, correr por cada coluna:

```sql
SELECT count(*) FROM <t> WHERE <c> IS NOT NULL AND <c> !~ '^\d{4}-\d{2}-\d{2}T';
```

Se der zero em todas, o `ALTER` passa. Se não der, essas linhas resolvem-se primeiro — e saber
quantas são vale mais que descobrir a meio.

**O `ALTER TABLE ... TYPE` reescreve a tabela** e leva um `ACCESS EXCLUSIVE`. Nas tabelas grandes
(`platform_logs`, `events`, `chunks`) isso é uma pausa, não um instante. Medir o tamanho antes e
fazer as grandes numa janela.

**Reversão:** o inverso é `USING <c>::text`, e o RFC3339 volta. Guardar o dump antes à mesma.

## Passos

1. Medir: linhas por tabela e o `count` do formato acima, em produção, sem alterar nada.
2. Ensaiar contra um dump restaurado — é aqui que se descobre o tempo real de cada `ALTER`.
3. Migração por lotes: as tabelas pequenas primeiro, as grandes numa janela.
4. Código: structs, queries, `now_rfc3339()` → `now()`.
5. Apagar a regra do `CLAUDE.md` e o `timeutil::now_rfc3339`.

## Não fazer

Não converter e reescrever o código na mesma migração. As colunas convertidas continuam a ler-se
como texto por um `::text`, portanto **a migração de esquema pode ir sozinha** e o código segue
depois, ficheiro a ficheiro. Juntar as duas coisas faz um MR que ninguém consegue rever.
