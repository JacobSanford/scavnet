//! The planned shape of one airing: preroll hiss, items separated by gap hiss, postroll hiss.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Hiss,
    /// Index into the conversation's items.
    Item(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub segment: Segment,
    pub start: f32,
    pub end: f32,
}

impl Span {
    pub fn seconds(&self) -> f32 {
        self.end - self.start
    }
}

#[derive(Clone, Debug)]
pub struct Timeline {
    spans: Vec<Span>,
}

impl Timeline {
    /// `items` holds each item's duration and the gap that follows it. The last gap is ignored.
    pub fn build(preroll: f32, items: &[(f32, f32)], postroll: f32) -> Self {
        let mut spans = Vec::with_capacity(items.len() * 2 + 1);
        let mut t = 0.0;
        let mut push = |segment, seconds: f32| {
            if seconds > 0.0 {
                spans.push(Span {
                    segment,
                    start: t,
                    end: t + seconds,
                });
                t += seconds;
            }
        };
        push(Segment::Hiss, preroll);
        for (i, &(duration, gap)) in items.iter().enumerate() {
            push(Segment::Item(i), duration);
            if i + 1 < items.len() {
                push(Segment::Hiss, gap);
            }
        }
        push(Segment::Hiss, postroll);
        Self { spans }
    }

    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    #[cfg(test)]
    pub fn total(&self) -> f32 {
        self.spans.last().map_or(0.0, |s| s.end)
    }

    /// The span playing at `t` seconds, with its index.
    pub fn at(&self, t: f32) -> Option<(usize, &Span)> {
        let i = self.spans.partition_point(|s| s.end <= t);
        self.spans.get(i).filter(|s| t >= s.start).map(|s| (i, s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_skips_last_gap_and_empty_spans() {
        let t = Timeline::build(1.0, &[(2.0, 0.5), (3.0, 0.0), (1.0, 9.0)], 0.25);
        let segs: Vec<_> = t.spans().iter().map(|s| s.segment).collect();
        assert_eq!(
            segs,
            [
                Segment::Hiss,
                Segment::Item(0),
                Segment::Hiss,
                Segment::Item(1),
                Segment::Item(2),
                Segment::Hiss
            ]
        );
        assert_eq!(t.total(), 1.0 + 2.0 + 0.5 + 3.0 + 1.0 + 0.25);
    }

    #[test]
    fn lookup_by_time() {
        let t = Timeline::build(1.0, &[(2.0, 0.0)], 1.0);
        assert_eq!(t.at(0.0).unwrap().1.segment, Segment::Hiss);
        assert_eq!(t.at(1.0).unwrap().1.segment, Segment::Item(0));
        assert_eq!(t.at(2.999).unwrap().0, 1);
        assert_eq!(t.at(3.0).unwrap().0, 2);
        assert!(t.at(4.0).is_none());
        assert!(t.at(-0.1).is_none());
    }
}
