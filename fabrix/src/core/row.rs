//! Fabrix row
//!
//! This module contains the row struct, which is used to represent a row in a DataFrame.
//!
//! Methods:
//! 1. new
//! 1. from_values
//! 1. data
//! 1. index
//! 1. data_fields
//! 1. len
//!
//! Methods provided to Dataframe:
//! 1. from_rows
//! 1. from_row_values_iter
//! 1. from_row_values
//! 1. get_row_by_idx
//! 1. get_row
//! 1. append
//! 1. insert_row_by_idx
//! 1. insert_row
//! 1. insert_rows_by_idx
//! 1. insert_rows

use itertools::Itertools;
use polars::prelude::Field;

use super::{inf_err, oob_err, util::Stepper, SeriesIntoIterator};
use crate::{CoreError, CoreResult, D2Value, DataFrame, Series, Value, ValueType};

#[derive(Debug, Clone)]
pub struct Row {
    pub(crate) index: Value,
    pub(crate) data: Vec<Value>,
}

impl Row {
    /// Row constructor
    pub fn new(index: Value, data: Vec<Value>) -> Self {
        Row { index, data }
    }

    /// Row constructor, no index
    pub fn from_values(data: Vec<Value>) -> Self {
        Row {
            index: Value::Null,
            data,
        }
    }

    /// get data
    pub fn data(&self) -> &[Value] {
        &self.data[..]
    }

    /// get index
    pub fn index(&self) -> &Value {
        &self.index
    }

    /// get index type
    pub fn index_dtype(&self) -> ValueType {
        ValueType::from(&self.index)
    }

    /// get data field
    pub fn data_fields(&self) -> Vec<Field> {
        self.data.iter().map(|v| v.into()).collect()
    }

    /// check if the row is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// row length
    pub fn len(&self) -> usize {
        self.data.len()
    }
}

impl DataFrame {
    /// create a DataFrame by Rows, slower than column-wise constructors.
    /// cannot build from an empty `Vec<Row>`
    pub fn from_rows(rows: Vec<Row>) -> CoreResult<Self> {
        let mut rows = rows;
        // rows length
        let m = rows.len();
        if m == 0 {
            return Err(CoreError::new_empty_error());
        }
        // rows width
        let n = rows.first().unwrap().len();
        let mut series = Vec::with_capacity(n);
        for j in 0..n {
            let mut buf = Vec::with_capacity(m);
            for r in rows.iter_mut() {
                let mut tmp = Value::Null;
                std::mem::swap(&mut tmp, &mut r.data[j]);
                buf.push(tmp);
            }
            series.push(Series::from_values(buf, &format!("Column_{:?}", j), true)?);
        }
        let index = rows.iter().map(|r| r.index.clone()).collect();

        DataFrame::from_series(series, Series::from_values_default_name(index, true)?)
    }

    /// create a DataFrame by IntoIter<Vec<Value>>, slower than column-wise constructors
    /// cannot build from an empty iterator
    ///
    /// index_col: the index column location, default is None
    pub fn from_row_values_iter(
        iter: std::vec::IntoIter<Vec<Value>>,
        index_col: Option<usize>,
    ) -> CoreResult<Self> {
        // create a peekable iterator
        let mut iter = iter.peekable();

        if iter.peek().is_none() {
            return Err(CoreError::new_empty_error());
        }

        // length of the first row, and width of the dataframe. number of columns
        let n = iter.peek().unwrap().len();
        let mut transposed_values: D2Value = vec![vec![]; n];

        for row in iter {
            row.into_iter()
                .enumerate()
                .for_each(|(i, v)| transposed_values[i].push(v));
        }

        // take an index series from the `transposed_values` if index_col is not None
        let index_series = index_col
            .and_then(|i| {
                // if index_col is out of range, simply ignore it and the dataframe will use the default index
                if i >= n {
                    None
                } else {
                    // take the index column, and remove it from the `transposed_values`
                    let v = transposed_values.remove(i);
                    Some(Series::from_values(v, "index", true))
                }
            })
            .transpose()?;

        // from the `transposed_values` to a vec of series
        let series = transposed_values
            .into_iter()
            .enumerate()
            .map(|(i, v)| Series::from_values(v, &format!("Column_{:?}", i), true))
            .collect::<CoreResult<Vec<_>>>()?;

        match index_series {
            Some(s) => DataFrame::from_series(series, s),
            None => DataFrame::from_series_default_index(series),
        }
    }

    /// create a DataFrame by D2Value, slower than column-wise constructors
    pub fn from_row_values(values: D2Value, index_col: Option<usize>) -> CoreResult<Self> {
        let iter = values.into_iter();
        DataFrame::from_row_values_iter(iter, index_col)
    }

    /// get a row by idx. This method is slower than get a column (`self.data.get_row`).
    /// beware performance: `Series.get` is slow.
    pub fn get_row_by_idx(&self, idx: usize) -> CoreResult<Row> {
        let len = self.height();
        if idx >= len {
            return Err(oob_err(idx, len));
        }
        let (data, index) = (
            self.data
                .iter()
                .map(|s| {
                    let val: Value = s.get(idx).into();
                    val
                })
                .collect_vec(),
            self.index.get(idx)?,
        );

        Ok(Row { index, data })
    }

    /// get a row by index. This method is slower than get a column.
    pub fn get_row(&self, index: &Value) -> CoreResult<Row> {
        self.index
            .find_index(index)
            .map_or(Err(inf_err(index)), |i| self.get_row_by_idx(i))
    }

    /// append a row to the dataframe. dtypes of the row must be equivalent to self dtypes
    pub fn append(&mut self, row: Row) -> CoreResult<&mut Self> {
        let mut d = DataFrame::from_rows(vec![row])?;
        d.set_column_names(&self.get_column_names())?;
        self.vconcat_mut(&d)
    }

    /// insert a row into the dataframe by idx
    pub fn insert_row_by_idx(&mut self, idx: usize, row: Row) -> CoreResult<&mut Self> {
        let len = self.height();
        let mut d1 = self.slice(0, idx);
        let d2 = self.slice(idx as i64, len);

        d1.append(row)?.vconcat_mut(&d2)?;
        *self = d1;

        Ok(self)
    }

    /// insert a row into the dataframe
    pub fn insert_row(&mut self, index: Value, row: Row) -> CoreResult<&mut Self> {
        match self.index.find_index(&index) {
            Some(idx) => self.insert_row_by_idx(idx, row),
            None => Err(inf_err(&index)),
        }
    }

    /// insert rows into the dataframe by idx
    pub fn insert_rows_by_idx(&mut self, idx: usize, rows: Vec<Row>) -> CoreResult<&mut Self> {
        let len = self.height();
        let mut d1 = self.slice(0, idx);
        let d2 = self.slice(idx as i64, len);
        let mut di = DataFrame::from_rows(rows)?;
        di.set_column_names(&self.get_column_names())?;

        d1.vconcat_mut(&di)?.vconcat_mut(&d2)?;
        *self = d1;

        Ok(self)
    }

    /// insert rows into the dataframe by index
    pub fn insert_rows(&mut self, index: Value, rows: Vec<Row>) -> CoreResult<&mut Self> {
        match self.index.find_index(&index) {
            Some(idx) => self.insert_rows_by_idx(idx, rows),
            None => Err(inf_err(&index)),
        }
    }
}

impl IntoIterator for DataFrame {
    type Item = Row;
    type IntoIter = DataFrameIntoIterator;

    fn into_iter(self) -> Self::IntoIter {
        let len = self.height();

        let mut data_iters = Vec::with_capacity(self.width() + 1);
        for s in self.data.iter() {
            let iter = Series(s.clone()).into_iter();
            data_iters.push(iter);
        }

        DataFrameIntoIterator {
            index_iter: self.index.into_iter(),
            data_iters,
            stepper: Stepper::new(len),
        }
    }
}

pub struct DataFrameIntoIterator {
    index_iter: SeriesIntoIterator,
    data_iters: Vec<SeriesIntoIterator>,
    stepper: Stepper,
}

impl Iterator for DataFrameIntoIterator {
    type Item = Row;

    fn next(&mut self) -> Option<Self::Item> {
        if self.stepper.exhausted() {
            None
        } else {
            let index = self.index_iter.next().unwrap();
            let data = self
                .data_iters
                .iter_mut()
                .map(|v| v.next().unwrap())
                .collect::<Vec<_>>();

            self.stepper.forward();
            Some(Row::new(index, data))
        }
    }
}

#[cfg(test)]
mod test_row {

    use crate::{df, rows, value, DataFrame, Row};

    #[test]
    fn test_from_rows() {
        let rows = rows!(
            [0, "Jacob", "A", 10],
            [1, "Sam", "A", 9],
            [2, "James", "A", 9],
        );

        let df = DataFrame::from_rows(rows);
        assert!(df.is_ok());
        assert!(df.unwrap().shape() == (3, 4));

        let rows = rows!(
            100 => [0, "Jacob", "A", 10],
            101 => [1, "Sam", "A", 9],
            102 => [2, "James", "A", 9],
        );

        let df = DataFrame::from_rows(rows);
        assert!(df.is_ok());
        let df = df.unwrap();
        assert!(df.shape() == (3, 4));
        assert!(df.index().len() == 3);
    }

    #[test]
    fn test_from_vec_vec_value() {
        let vvv = vec![
            vec![value!(11), value!("Jacob"), value!("A"), value!(10)],
            vec![value!(21), value!("Sam"), value!("A"), value!(9)],
            vec![value!(31), value!("James"), value!("A"), value!(9)],
        ];

        let df = DataFrame::from_row_values(vvv, None);
        assert!(df.is_ok());

        let df = df.unwrap();
        assert!(df.shape() == (3, 4));
    }

    #[test]
    fn test_get_row() {
        let df = df![
            "ord";
            "names" => ["Jacob", "Sam", "James"],
            "ord" => [1,2,3],
            "val" => [Some(10), None, Some(8)]
        ];
        assert!(df.is_ok());

        let df = df.unwrap();
        assert_eq!(df.shape(), (3, 2));

        let test1 = df.get_row_by_idx(1).unwrap();
        assert_eq!(test1.index(), &value!(2));
        assert_eq!(test1.data(), &[value!("Sam"), value!(None::<i32>)]);

        let test2 = df.get_row(&value!(2i32)).unwrap();
        assert_eq!(test2.index(), &value!(2));
        assert_eq!(test2.data(), &[value!("Sam"), value!(None::<i32>)]);
    }

    #[test]
    fn test_df_op() {
        let mut df = df![
            "ord";
            "names" => ["Jacob", "Sam", "James"],
            "ord" => [1,2,3],
            "val" => [10, 9, 8]
        ]
        .unwrap();

        let row1 = Row::new(value!(4), vec![value!("Mia"), value!(10)]);
        let res1 = df.append(row1);
        assert!(res1.is_ok());

        let row2 = Row::new(value!(5), vec![value!("Mandy"), value!(9)]);
        let res2 = df.insert_row(value!(2), row2);
        assert!(res2.is_ok());
        assert!(df.shape() == (5, 2));

        let rows = rows!(
            6 => ["Jamie", 9],
            7 => ["Justin", 6],
            8 => ["Julia", 8]
        );

        let res3 = df.insert_rows(value!(5), rows);
        assert!(res3.is_ok());

        let res4 = df.remove_row(value!(7));
        assert!(res4.is_ok());
        assert_eq!(df.shape(), (7, 2));

        let res4 = df.remove_slice(1, 2);
        assert!(res4.is_ok());
        assert_eq!(df.shape(), (5, 2));

        let res5 = df.remove_rows(vec![value!(2), value!(4)]);
        assert!(res5.is_ok());
        assert_eq!(df.shape(), (3, 2));
    }

    #[test]
    fn test_df_iter() {
        let df = df![
            "name" => ["Jacob", "Sam", "James", "Julia"],
            "star" => [100, 99, 100, 69],
            "loc" => [2u8, 3, 1, 4]
        ]
        .unwrap();

        let mut iter = df.into_iter();

        let r1 = iter.next();
        assert!(r1.is_some());
        assert_eq!(
            r1.unwrap().data(),
            vec![value!("Jacob"), value!(100), value!(2u8)]
        );

        let r2 = iter.next();
        assert!(r2.is_some());
        assert_eq!(
            r2.unwrap().data(),
            vec![value!("Sam"), value!(99), value!(3u8)]
        );

        let r3 = iter.next();
        assert!(r3.is_some());
        assert_eq!(
            r3.unwrap().data(),
            vec![value!("James"), value!(100), value!(1u8)]
        );

        let r4 = iter.next();
        assert!(r4.is_some());
        assert_eq!(
            r4.unwrap().data(),
            vec![value!("Julia"), value!(69), value!(4u8)]
        );

        let r5 = iter.next();
        assert!(r5.is_none());
    }
}
