use anyhow::{Context, Result};
use arrow::array::*;
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::metadata::KeyValue;
use parquet::file::properties::WriterProperties;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::thetadata_hist::{CachedBar, CachedGexEntry, CachedWallEntry};

// ─── Constants ──────────────────────────────────────────────────────────────

const NARROW_N: usize = 5;
const WIDE_N: usize = 5;

/// Scalar columns before wall strike/γ×OI pairs (v1: 12, v2: +cw_depth, v3: +gamma_tilt).
const GEX_SCALAR_COLS_V1: usize = 12;
const GEX_SCALAR_COLS_V2: usize = 13;
const GEX_SCALAR_COLS_V3: usize = 14;
const GEX_WALL_PAIR_COLS: usize = (NARROW_N + NARROW_N + WIDE_N + WIDE_N) * 2;

// ─── Bars ───────────────────────────────────────────────────────────────────

fn bars_schema() -> Schema {
    Schema::new(vec![
        Field::new("ts", DataType::Int64, false),
        Field::new("o", DataType::Float64, false),
        Field::new("h", DataType::Float64, false),
        Field::new("l", DataType::Float64, false),
        Field::new("c", DataType::Float64, false),
        Field::new("v", DataType::Float64, false),
    ])
}

pub fn write_bars(path: &Path, bars: &[CachedBar], processed_dates: &HashSet<String>) -> Result<()> {
    let schema = Arc::new(bars_schema());
    let ts: Vec<i64> = bars.iter().map(|b| b.timestamp).collect();
    let o: Vec<f64> = bars.iter().map(|b| b.open).collect();
    let h: Vec<f64> = bars.iter().map(|b| b.high).collect();
    let l: Vec<f64> = bars.iter().map(|b| b.low).collect();
    let c: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let v: Vec<f64> = bars.iter().map(|b| b.volume).collect();

    let batch = arrow::record_batch::RecordBatch::try_new(schema.clone(), vec![
        Arc::new(Int64Array::from(ts)),
        Arc::new(Float64Array::from(o)),
        Arc::new(Float64Array::from(h)),
        Arc::new(Float64Array::from(l)),
        Arc::new(Float64Array::from(c)),
        Arc::new(Float64Array::from(v)),
    ]).context("build bars batch")?;

    let csv = set_to_csv(processed_dates);
    write_parquet(path, schema, &[batch], &[("processed_dates", &csv)])
}

pub fn read_bars(path: &Path) -> Option<Vec<CachedBar>> {
    if !path.exists() { return None; }
    let batches = read_parquet(path).ok()?;
    let mut out = Vec::new();
    for batch in &batches {
        let ts = col_i64(batch, 0);
        let o = col_f64(batch, 1);
        let h = col_f64(batch, 2);
        let l = col_f64(batch, 3);
        let c = col_f64(batch, 4);
        let v = col_f64(batch, 5);
        for i in 0..batch.num_rows() {
            out.push(CachedBar {
                timestamp: ts.value(i),
                open: o.value(i), high: h.value(i), low: l.value(i),
                close: c.value(i), volume: v.value(i),
            });
        }
    }
    Some(out)
}

// ─── GEX ────────────────────────────────────────────────────────────────────

fn gex_schema() -> Schema {
    let mut fields = vec![
        Field::new("ts", DataType::Int64, false),
        Field::new("spot", DataType::Float64, false),
        Field::new("net_gex", DataType::Float64, false),
        Field::new("atm_put_iv", DataType::Float64, false),
        Field::new("pw_com_dist_pct", DataType::Float64, false),
        Field::new("pw_near_far_ratio", DataType::Float64, false),
        Field::new("atm_gamma_dominance", DataType::Float64, false),
        Field::new("near_gamma_imbalance", DataType::Float64, false),
        Field::new("total_put_goi", DataType::Float64, false),
        Field::new("total_call_goi", DataType::Float64, false),
        Field::new("cw_depth_ratio", DataType::Float64, false),
        Field::new("gamma_tilt", DataType::Float64, false),
        Field::new("net_vanna", DataType::Float64, false),
        Field::new("net_delta", DataType::Float64, false),
    ];
    for (prefix, n) in [
        ("pw", NARROW_N), ("cw", NARROW_N),
        ("wpw", WIDE_N), ("wcw", WIDE_N),
    ] {
        for i in 0..n {
            fields.push(Field::new(format!("{prefix}_s{i}"), DataType::Float64, false));
            fields.push(Field::new(format!("{prefix}_g{i}"), DataType::Float64, false));
        }
    }
    Schema::new(fields)
}

fn walls_to_flat(walls: &[CachedWallEntry], n: usize, strikes: &mut Vec<Vec<f64>>, gois: &mut Vec<Vec<f64>>) {
    for i in 0..n {
        let (s, g) = walls.get(i).map(|w| (w.strike, w.gamma_oi)).unwrap_or((0.0, 0.0));
        strikes[i].push(s);
        gois[i].push(g);
    }
}


pub fn write_gex(path: &Path, entries: &[CachedGexEntry], processed_months: &HashSet<String>) -> Result<()> {
    let schema = Arc::new(gex_schema());
    let n = entries.len();

    let ts: Vec<i64> = entries.iter().map(|e| e.timestamp).collect();
    let spot: Vec<f64> = entries.iter().map(|e| e.spot).collect();
    let net_gex: Vec<f64> = entries.iter().map(|e| e.net_gex).collect();
    let atm_put_iv: Vec<f64> = entries.iter().map(|e| e.atm_put_iv.unwrap_or(0.0)).collect();
    let pw_com: Vec<f64> = entries.iter().map(|e| e.pw_com_dist_pct).collect();
    let pw_nfr: Vec<f64> = entries.iter().map(|e| e.pw_near_far_ratio).collect();
    let atm_gd: Vec<f64> = entries.iter().map(|e| e.atm_gamma_dominance).collect();
    let ngi: Vec<f64> = entries.iter().map(|e| e.near_gamma_imbalance).collect();
    let tp_goi: Vec<f64> = entries.iter().map(|e| e.total_put_goi).collect();
    let tc_goi: Vec<f64> = entries.iter().map(|e| e.total_call_goi).collect();
    let cw_depth: Vec<f64> = entries.iter().map(|e| e.cw_depth_ratio).collect();
    let g_tilt: Vec<f64> = entries.iter().map(|e| e.gamma_tilt).collect();
    let nv: Vec<f64> = entries.iter().map(|e| e.net_vanna).collect();
    let nd: Vec<f64> = entries.iter().map(|e| e.net_delta).collect();

    let mut columns: Vec<Arc<dyn Array>> = vec![
        Arc::new(Int64Array::from(ts)),
        Arc::new(Float64Array::from(spot)),
        Arc::new(Float64Array::from(net_gex)),
        Arc::new(Float64Array::from(atm_put_iv)),
        Arc::new(Float64Array::from(pw_com)),
        Arc::new(Float64Array::from(pw_nfr)),
        Arc::new(Float64Array::from(atm_gd)),
        Arc::new(Float64Array::from(ngi)),
        Arc::new(Float64Array::from(tp_goi)),
        Arc::new(Float64Array::from(tc_goi)),
        Arc::new(Float64Array::from(cw_depth)),
        Arc::new(Float64Array::from(g_tilt)),
        Arc::new(Float64Array::from(nv)),
        Arc::new(Float64Array::from(nd)),
    ];

    let wall_groups: Vec<(fn(&CachedGexEntry) -> &Vec<CachedWallEntry>, usize)> = vec![
        (|e| &e.put_walls, NARROW_N),
        (|e| &e.call_walls, NARROW_N),
        (|e| &e.wide_put_walls, WIDE_N),
        (|e| &e.wide_call_walls, WIDE_N),
    ];
    for (walls_fn, sz) in wall_groups {
        let mut strike_cols: Vec<Vec<f64>> = (0..sz).map(|_| Vec::with_capacity(n)).collect();
        let mut goi_cols: Vec<Vec<f64>> = (0..sz).map(|_| Vec::with_capacity(n)).collect();
        for e in entries {
            walls_to_flat(walls_fn(e), sz, &mut strike_cols, &mut goi_cols);
        }
        for i in 0..sz {
            columns.push(Arc::new(Float64Array::from(std::mem::take(&mut strike_cols[i]))));
            columns.push(Arc::new(Float64Array::from(std::mem::take(&mut goi_cols[i]))));
        }
    }

    let batch = arrow::record_batch::RecordBatch::try_new(schema.clone(), columns)
        .context("build gex batch")?;
    let csv = set_to_csv(processed_months);
    write_parquet(path, schema, &[batch], &[("processed_months", &csv)])
}

pub fn read_gex(path: &Path) -> Option<Vec<CachedGexEntry>> {
    if !path.exists() { return None; }
    let batches = read_parquet(path).ok()?;
    let mut out = Vec::new();
    for batch in &batches {
        let n_rows = batch.num_rows();
        let ncols = batch.num_columns();
        let schema_ver = match ncols {
            n if n == GEX_SCALAR_COLS_V1 + GEX_WALL_PAIR_COLS => 1,
            n if n == GEX_SCALAR_COLS_V2 + GEX_WALL_PAIR_COLS => 2,
            n if n == GEX_SCALAR_COLS_V3 + GEX_WALL_PAIR_COLS => 3,
            _ => continue,
        };
        let scalar_cols = match schema_ver { 1 => GEX_SCALAR_COLS_V1, 2 => GEX_SCALAR_COLS_V2, _ => GEX_SCALAR_COLS_V3 };

        let ts = col_i64(batch, 0);
        let spot = col_f64(batch, 1);
        let net_gex = col_f64(batch, 2);
        let atm_iv = col_f64(batch, 3);
        let pw_com = col_f64(batch, 4);
        let pw_nfr = col_f64(batch, 5);
        let atm_gd = col_f64(batch, 6);
        let ngi = col_f64(batch, 7);
        let tp_goi = col_f64(batch, 8);
        let tc_goi = col_f64(batch, 9);
        let cw_depth_arr = if schema_ver >= 2 { Some(col_f64(batch, 10)) } else { None };
        let gt_arr = if schema_ver >= 3 { Some(col_f64(batch, 11)) } else { None };
        let nv_idx = match schema_ver { 1 => 10, 2 => 11, _ => 12 };
        let nd_idx = nv_idx + 1;
        let nv_col = col_f64(batch, nv_idx);
        let nd_col = col_f64(batch, nd_idx);

        for row in 0..n_rows {
            let iv = atm_iv.value(row);
            let cw_depth_ratio = cw_depth_arr.map(|a| a.value(row)).unwrap_or(0.0);
            let gamma_tilt = gt_arr.map(|a| a.value(row)).unwrap_or(0.0);
            let nv = nv_col.value(row);
            let nd = nd_col.value(row);
            let mut col = scalar_cols;
            let pw = read_walls(batch, &mut col, NARROW_N, row);
            let cw = read_walls(batch, &mut col, NARROW_N, row);
            let wpw = read_walls(batch, &mut col, WIDE_N, row);
            let wcw = read_walls(batch, &mut col, WIDE_N, row);

            out.push(CachedGexEntry {
                timestamp: ts.value(row),
                spot: spot.value(row),
                net_gex: net_gex.value(row),
                atm_put_iv: if iv != 0.0 { Some(iv) } else { None },
                put_walls: pw, call_walls: cw,
                wide_put_walls: wpw, wide_call_walls: wcw,
                pw_com_dist_pct: pw_com.value(row),
                pw_near_far_ratio: pw_nfr.value(row),
                atm_gamma_dominance: atm_gd.value(row),
                near_gamma_imbalance: ngi.value(row),
                total_put_goi: tp_goi.value(row),
                total_call_goi: tc_goi.value(row),
                cw_depth_ratio,
                gamma_tilt,
                net_vanna: nv,
                net_delta: nd,
            });
        }
    }
    Some(out)
}

fn read_walls(batch: &arrow::record_batch::RecordBatch, col: &mut usize, n: usize, row: usize) -> Vec<CachedWallEntry> {
    let mut walls = Vec::with_capacity(n);
    for _ in 0..n {
        let s = col_f64(batch, *col).value(row);
        let g = col_f64(batch, *col + 1).value(row);
        *col += 2;
        if s != 0.0 || g != 0.0 {
            walls.push(CachedWallEntry { strike: s, gamma_oi: g });
        }
    }
    walls
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn col_i64(batch: &arrow::record_batch::RecordBatch, idx: usize) -> &Int64Array {
    batch.column(idx).as_any().downcast_ref::<Int64Array>().expect("i64 column")
}

fn col_f64(batch: &arrow::record_batch::RecordBatch, idx: usize) -> &Float64Array {
    batch.column(idx).as_any().downcast_ref::<Float64Array>().expect("f64 column")
}

fn write_parquet(
    path: &Path,
    schema: Arc<Schema>,
    batches: &[arrow::record_batch::RecordBatch],
    kv: &[(&str, &str)],
) -> Result<()> {
    let tmp = path.with_extension("parquet.tmp");
    let file = fs::File::create(&tmp)?;
    let mut builder = WriterProperties::builder()
        .set_compression(Compression::ZSTD(Default::default()));
    for &(k, v) in kv {
        builder = builder.set_key_value_metadata(Some(vec![
            KeyValue::new(k.to_string(), v.to_string()),
        ]));
    }
    let mut writer = ArrowWriter::try_new(file, schema, Some(builder.build()))?;
    for batch in batches {
        writer.write(batch)?;
    }
    writer.close()?;
    fs::rename(&tmp, path).context("rename parquet tmp")?;
    Ok(())
}

fn read_parquet(path: &Path) -> Result<Vec<arrow::record_batch::RecordBatch>> {
    let file = fs::File::open(path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?
        .build()?;
    let batches: Vec<_> = reader.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(batches)
}

/// Read a key-value metadata entry from a parquet file footer.
pub fn read_metadata_key(path: &Path, key: &str) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).ok()?;
    let meta = builder.metadata().file_metadata().key_value_metadata()?;
    meta.iter().find(|kv| kv.key == key)?.value.clone()
}

/// Read a comma-separated set from parquet footer metadata.
pub fn read_metadata_set(path: &Path, key: &str) -> HashSet<String> {
    match read_metadata_key(path, key) {
        Some(v) if !v.is_empty() => v.split(',').map(|s| s.to_string()).collect(),
        _ => HashSet::new(),
    }
}

fn set_to_csv(set: &HashSet<String>) -> String {
    let mut v: Vec<&str> = set.iter().map(|s| s.as_str()).collect();
    v.sort_unstable();
    v.join(",")
}
