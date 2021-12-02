//! impl FnMut for sync fn & async fn

use futures::future::LocalBoxFuture;

fn main() {
    unimplemented!();
}

struct Foo
where
    Self: Send;

impl Foo {
    fn convert1(&mut self, data: &[u8]) -> Result<usize, ()> {
        Ok(data.len())
    }

    async fn consume1<'a>(&self, data: &[u8]) -> Result<usize, ()> {
        Ok(data.len())
    }

    fn convert2(&mut self, data: &[u8]) -> Result<usize, ()> {
        Ok(data.len() * 10)
    }

    async fn consume2<'a>(&mut self, data: &[u8]) -> Result<usize, ()> {
        Ok(data.len() * 10)
    }
}

struct Bar;

impl Bar {
    async fn process(
        &mut self,
        d1: &[u8],
        d2: &[u8],
        mut convert_fn: impl FnMut(&[u8]) -> Result<usize, ()>,
        mut consume_fn: impl FnMut(&[u8]) -> LocalBoxFuture<Result<usize, ()>>,
    ) -> Result<usize, ()> {
        let a = convert_fn(d1).unwrap();

        let b = consume_fn(d2).await.unwrap();

        Ok(a + b)
    }
}

// notice that async fn `consume_fn` cannot borrow value from the outer scope
// error: lifetime may not live long enough closure implements `FnMut`,
// so references to captured variables can't escape the closure
#[tokio::test]
async fn use_fn_mut_test() {
    let mut bar = Bar;

    let d1 = [1, 2, 3];
    let d2 = [4, 5, 6];

    let mut foo = Foo;

    let res1 = bar
        .process(
            &d1,
            &d2,
            |d| foo.convert1(d),
            |d| Box::pin(async { Foo.consume1(d).await }),
        )
        .await;

    println!("{:?}", res1);

    let res2 = bar
        .process(
            &d1,
            &d2,
            |d| foo.convert2(d),
            |d| Box::pin(async { Foo.consume2(d).await }),
        )
        .await;

    println!("{:?}", res2);
}
