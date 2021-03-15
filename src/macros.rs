macro_rules! if_miri {
    ($lhs: expr, $rhs: expr) => {
        if cfg!(miri) {
            $lhs
        } else {
            $rhs
        }
    };
}

#[cfg(test)]
mod test {
    #[test]
    fn test_if_miri() {
        let i: i32 = if_miri!(100, 1000);
        #[cfg(miri)]
        assert_eq!(i, 100);
        #[cfg(not(miri))]
        assert_eq!(i, 1000);
    }
}
