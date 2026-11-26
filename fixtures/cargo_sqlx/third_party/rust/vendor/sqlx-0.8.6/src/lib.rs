pub fn driver() -> &'static str {
    #[cfg(feature = "postgres")]
    return sqlx_postgres::NAME;
    #[cfg(not(feature = "postgres"))]
    return "none";
}
