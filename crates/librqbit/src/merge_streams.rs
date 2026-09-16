use futures::stream::Stream;

pub fn merge_streams<
    I,
    S1: Stream<Item = I> + 'static + Unpin,
    S2: Stream<Item = I> + 'static + Unpin,
>(
    s1: S1,
    s2: S2,
) -> impl Stream<Item = I> + Unpin + 'static {
    use tokio_stream::StreamExt;
    s1.merge(s2)
}

#[cfg(test)]
mod tests {
    use futures::stream;
    use tokio_stream::StreamExt;

    use super::merge_streams;

    #[tokio::test]
    async fn yields_every_item_from_both_streams() {
        let mut values = merge_streams(stream::iter([1, 3]), stream::iter([2, 4]))
            .collect::<Vec<_>>()
            .await;
        values.sort_unstable();
        assert_eq!(values, vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn handles_empty_streams() {
        let values = merge_streams(stream::empty::<u8>(), stream::iter([1, 2]))
            .collect::<Vec<_>>()
            .await;
        assert_eq!(values, vec![1, 2]);
    }
}
