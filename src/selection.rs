//! Geometria da seleção de área na tela: alças, mover e redimensionar.

/// Retângulo de seleção em coordenadas de tela, sempre normalizado
/// (`x0 <= x1`, `y0 <= y1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sel {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// Alça: 0 a 3 são os cantos (sup. esq., sup. dir., inf. dir., inf. esq.),
    /// 4 a 7 os meios dos lados (cima, direita, baixo, esquerda).
    Handle(usize),
    Inside,
    Outside,
}

impl Sel {
    pub fn from_points(a: (i32, i32), b: (i32, i32)) -> Self {
        Self {
            x0: a.0.min(b.0),
            y0: a.1.min(b.1),
            x1: a.0.max(b.0),
            y1: a.1.max(b.1),
        }
    }

    pub fn width(&self) -> i32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> i32 {
        self.y1 - self.y0
    }

    pub fn handles(&self) -> [(i32, i32); 8] {
        let (cx, cy) = ((self.x0 + self.x1) / 2, (self.y0 + self.y1) / 2);
        [
            (self.x0, self.y0),
            (self.x1, self.y0),
            (self.x1, self.y1),
            (self.x0, self.y1),
            (cx, self.y0),
            (self.x1, cy),
            (cx, self.y1),
            (self.x0, cy),
        ]
    }

    /// O que está sob o ponto `p`. Alças têm prioridade sobre o interior.
    pub fn hit(&self, p: (i32, i32), tol: i32) -> Hit {
        if let Some(i) = self
            .handles()
            .iter()
            .position(|h| (h.0 - p.0).abs() <= tol && (h.1 - p.1).abs() <= tol)
        {
            return Hit::Handle(i);
        }
        if (self.x0..=self.x1).contains(&p.0) && (self.y0..=self.y1).contains(&p.1) {
            Hit::Inside
        } else {
            Hit::Outside
        }
    }

    /// Desloca sem sair da tela (`bounds` = largura e altura da tela).
    pub fn translate_clamped(&self, dx: i32, dy: i32, bounds: (i32, i32)) -> Self {
        let dx = dx.clamp(-self.x0, bounds.0 - self.x1);
        let dy = dy.clamp(-self.y0, bounds.1 - self.y1);
        Self {
            x0: self.x0 + dx,
            y0: self.y0 + dy,
            x1: self.x1 + dx,
            y1: self.y1 + dy,
        }
    }

    /// Resultado de arrastar a alça `idx` até `p`.
    pub fn drag_handle(&self, idx: usize, p: (i32, i32)) -> Self {
        let corners = [
            (self.x0, self.y0),
            (self.x1, self.y0),
            (self.x1, self.y1),
            (self.x0, self.y1),
        ];
        match idx {
            0..=3 => Self::from_points(corners[(idx + 2) % 4], p),
            4 => Self::from_points((self.x0, p.1), (self.x1, self.y1)),
            5 => Self::from_points((self.x0, self.y0), (p.0, self.y1)),
            6 => Self::from_points((self.x0, self.y0), (self.x1, p.1)),
            _ => Self::from_points((p.0, self.y0), (self.x1, self.y1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel() -> Sel {
        Sel::from_points((100, 100), (300, 200))
    }

    #[test]
    fn normaliza_pontos_em_qualquer_ordem() {
        assert_eq!(Sel::from_points((300, 200), (100, 100)), sel());
    }

    #[test]
    fn alca_tem_prioridade_sobre_o_interior() {
        let s = sel();
        assert_eq!(s.hit((102, 98), 6), Hit::Handle(0));
        assert_eq!(s.hit((200, 100), 6), Hit::Handle(4), "meio de cima");
        assert_eq!(s.hit((300, 150), 6), Hit::Handle(5), "meio da direita");
        assert_eq!(s.hit((150, 150), 6), Hit::Inside);
        assert_eq!(s.hit((50, 50), 6), Hit::Outside);
    }

    #[test]
    fn mover_nao_sai_da_tela() {
        let s = sel();
        assert_eq!(
            s.translate_clamped(10, 20, (1000, 1000)),
            Sel::from_points((110, 120), (310, 220))
        );
        assert_eq!(
            s.translate_clamped(-500, -500, (1000, 1000)),
            Sel::from_points((0, 0), (200, 100))
        );
        assert_eq!(
            s.translate_clamped(5000, 5000, (1000, 800)),
            Sel::from_points((800, 700), (1000, 800))
        );
    }

    #[test]
    fn alcas_redimensionam_como_no_editor() {
        let s = sel();
        assert_eq!(
            s.drag_handle(2, (400, 260)),
            Sel::from_points((100, 100), (400, 260))
        );
        assert_eq!(
            s.drag_handle(0, (50, 60)),
            Sel::from_points((50, 60), (300, 200))
        );
        assert_eq!(
            s.drag_handle(4, (999, 80)),
            Sel::from_points((100, 80), (300, 200))
        );
        assert_eq!(
            s.drag_handle(5, (350, 999)),
            Sel::from_points((100, 100), (350, 200))
        );
        assert_eq!(
            s.drag_handle(7, (400, 0)),
            Sel::from_points((300, 100), (400, 200)),
            "inverte ao passar do lado oposto"
        );
    }
}
