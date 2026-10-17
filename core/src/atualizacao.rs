//! Atualização do app: a máquina de estados, pura e testável. Quem baixa e instala de verdade
//! é o app (plugin updater do Tauri); aqui fica só a decisão do que fazer e quando.
//!
//! SemAtualizacao → Disponivel → Baixando → ProntaParaInstalar → AguardandoMomentoSeguro → Instalando
//!
//! Regra de ouro: só instala com o mundo em estado conhecido. Nunca enquanto este PC hospeda,
//! replica, sincroniza ou passa o host. Se for host, primeiro passa o host (ou fecha o mundo
//! com o envio à nuvem conferido); se isso não se confirmar, a atualização é cancelada.

use serde::Serialize;

/// Atualização encontrada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Oferta {
    pub versao: String,
    pub notas: Option<String>,
    /// Abaixo da versão mínima do grupo (ou marcada como obrigatória no `latest.json`).
    pub obrigatoria: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Estado {
    SemAtualizacao,
    /// Encontrada; o download em segundo plano ainda não terminou (ou falhou).
    Disponivel,
    Baixando,
    /// Baixada e conferida (assinatura); falta só instalar.
    ProntaParaInstalar,
    /// O usuário pediu para atualizar; esperando o mundo ficar seguro (ou passando o host).
    AguardandoMomentoSeguro,
    Instalando,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pedido {
    /// "Atualizar agora" (ou "Passar o host e atualizar").
    Agora,
    /// "Atualizar ao fechar": no Sair, faz o fluxo seguro e instala sem perguntar de novo.
    AoFechar,
}

/// O que este PC está fazendo com o mundo agora.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Momento {
    pub hospedando: bool,
    /// Fase diferente de parado/esperando a vez (abrindo, fechando, recuperando, manutenção).
    pub fase_ocupada: bool,
    pub replicando: bool,
    /// Troca de host em andamento (passando ou assumindo).
    pub trocando: bool,
    /// Enviando ou baixando o mundo.
    pub sincronizando: bool,
}

impl Momento {
    pub fn seguro(&self) -> bool {
        !(self.hospedando || self.fase_ocupada || self.replicando || self.trocando || self.sincronizando)
    }
}

/// Como a sessão terminou antes de instalar (só estes dois casos liberam a instalação).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FimSeguro {
    /// Este PC não estava hospedando e o mundo local não tem nada pendente.
    NadaAberto,
    /// Passou o host e um amigo confirmou que tem a versão final.
    AmigoConfirmou,
    /// Fechou o mundo e o envio final à nuvem terminou e foi conferido.
    NuvemConferida,
}

/// O que o app deve fazer depois de cada evento.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Efeito {
    Nada,
    /// Baixar em segundo plano (sem instalar).
    Baixar,
    /// Passar o host (ou fechar o mundo salvando), com prazo, e devolver em `host_passado`.
    PassarHost,
    /// Esperar o mundo ficar seguro; o app chama `momento_mudou` a cada mudança de status.
    EsperarMomento,
    Instalar,
    Avisar { titulo: String, corpo: String },
}

pub const ERRO_DOWNLOAD: &str = "Não deu para baixar a atualização agora. Vamos tentar de novo mais tarde.";
pub const ERRO_TROCA: &str = "Não deu para passar o host a tempo. O mundo está seguro e a atualização fica para depois.";
pub const ERRO_INSTALAR: &str = "Não deu para instalar a atualização agora. O app continua funcionando; tente de novo.";

#[derive(Debug, Clone, Serialize)]
pub struct Maquina {
    pub estado: Estado,
    pub versao_atual: String,
    pub oferta: Option<Oferta>,
    pub pedido: Option<Pedido>,
    pub baixado: u64,
    pub total: Option<u64>,
    /// Mensagem amigável do último problema (some na próxima tentativa que der certo).
    pub erro: Option<String>,
    /// Passando o host para poder instalar.
    pub passando_host: bool,
    /// Bytes da atualização já baixados e conferidos.
    pronta: bool,
}

impl Maquina {
    pub fn nova(versao_atual: &str) -> Self {
        Self {
            estado: Estado::SemAtualizacao,
            versao_atual: versao_atual.into(),
            oferta: None,
            pedido: None,
            baixado: 0,
            total: None,
            erro: None,
            passando_host: false,
            pronta: false,
        }
    }

    fn ocupada(&self) -> bool {
        matches!(self.estado, Estado::AguardandoMomentoSeguro | Estado::Instalando) || self.passando_host
    }

    /// Resultado de uma verificação. Falha (sem internet, servidor fora) é silenciosa: fica
    /// como está e tenta de novo na próxima.
    pub fn verificou(&mut self, r: Result<Option<Oferta>, String>) -> Efeito {
        match r {
            Err(_) => Efeito::Nada,
            Ok(None) => {
                if !self.ocupada() {
                    *self = Self { versao_atual: self.versao_atual.clone(), ..Self::nova("") };
                }
                Efeito::Nada
            }
            Ok(Some(o)) => {
                if self.oferta.as_ref().map(|a| a.versao == o.versao).unwrap_or(false) {
                    if let Some(a) = self.oferta.as_mut() {
                        a.obrigatoria = o.obrigatoria;
                        a.notas = o.notas;
                    }
                    // download anterior falhou: tenta de novo agora
                    if self.estado == Estado::Disponivel {
                        self.estado = Estado::Baixando;
                        return Efeito::Baixar;
                    }
                    return Efeito::Nada;
                }
                if self.ocupada() {
                    return Efeito::Nada; // termina o que começou; a nova fica para a próxima
                }
                self.oferta = Some(o);
                self.pronta = false;
                self.baixado = 0;
                self.total = None;
                self.erro = None;
                self.estado = Estado::Baixando;
                Efeito::Baixar
            }
        }
    }

    /// Marca a oferta como obrigatória (versão mínima do grupo) sem nova verificação.
    pub fn definir_obrigatoria(&mut self, obrigatoria: bool) {
        if let Some(o) = self.oferta.as_mut() {
            o.obrigatoria = obrigatoria;
        }
    }

    pub fn progresso(&mut self, baixado: u64, total: Option<u64>) {
        self.baixado = baixado;
        self.total = total;
    }

    pub fn baixou(&mut self, r: Result<(), String>, m: Momento) -> Efeito {
        match r {
            Ok(()) => {
                self.pronta = true;
                self.erro = None;
                self.estado = Estado::ProntaParaInstalar;
                if self.pedido == Some(Pedido::Agora) {
                    return self.tentar(m);
                }
                Efeito::Nada
            }
            Err(_) => {
                self.pronta = false;
                self.estado = Estado::Disponivel;
                self.erro = Some(ERRO_DOWNLOAD.into());
                if self.pedido.take() == Some(Pedido::Agora) {
                    return aviso("Atualização adiada", ERRO_DOWNLOAD);
                }
                Efeito::Nada
            }
        }
    }

    pub fn pedir(&mut self, p: Pedido, m: Momento) -> Efeito {
        if self.oferta.is_none() || self.estado == Estado::Instalando || self.passando_host {
            return Efeito::Nada;
        }
        self.pedido = Some(p);
        self.erro = None;
        if !self.pronta {
            if self.estado == Estado::Disponivel {
                self.estado = Estado::Baixando;
                return Efeito::Baixar;
            }
            return Efeito::Nada; // instala quando o download terminar (se for "agora")
        }
        match p {
            Pedido::Agora => self.tentar(m),
            Pedido::AoFechar => Efeito::Nada,
        }
    }

    /// O app está fechando: com "atualizar ao fechar" e a atualização pronta, segue o fluxo seguro.
    pub fn fechando(&mut self, m: Momento) -> Efeito {
        if self.pedido == Some(Pedido::AoFechar) && self.pronta && !self.ocupada() {
            return self.tentar(m);
        }
        Efeito::Nada
    }

    fn tentar(&mut self, m: Momento) -> Efeito {
        if m.seguro() {
            self.estado = Estado::Instalando;
            return Efeito::Instalar;
        }
        self.estado = Estado::AguardandoMomentoSeguro;
        if m.hospedando && !m.trocando {
            self.passando_host = true;
            return Efeito::PassarHost;
        }
        Efeito::EsperarMomento
    }

    /// Mudou o status do mundo: se estava esperando e ficou seguro, instala.
    pub fn momento_mudou(&mut self, m: Momento) -> Efeito {
        if self.estado == Estado::AguardandoMomentoSeguro && !self.passando_host {
            if m.seguro() {
                self.estado = Estado::Instalando;
                return Efeito::Instalar;
            }
            // virou host enquanto esperava (ex.: assumiu numa troca automática): passa de novo
            if m.hospedando && !m.trocando && !m.fase_ocupada_sem_host() {
                self.passando_host = true;
                return Efeito::PassarHost;
            }
        }
        Efeito::Nada
    }

    /// Resultado da troca de host / fechamento antes de instalar.
    pub fn host_passado(&mut self, r: Result<FimSeguro, String>, m: Momento) -> Efeito {
        self.passando_host = false;
        match r {
            Ok(_) if m.seguro() => {
                self.estado = Estado::Instalando;
                Efeito::Instalar
            }
            Ok(_) => Efeito::EsperarMomento,
            Err(_) => {
                self.estado = Estado::ProntaParaInstalar;
                self.pedido = None;
                self.erro = Some(ERRO_TROCA.into());
                aviso("Atualização cancelada", ERRO_TROCA)
            }
        }
    }

    /// Na última conferência antes de instalar, o mundo deixou de estar seguro: volta a esperar.
    pub fn voltar_a_esperar(&mut self) {
        if self.estado == Estado::Instalando {
            self.estado = Estado::AguardandoMomentoSeguro;
        }
    }

    pub fn instalacao_falhou(&mut self) -> Efeito {
        self.estado = if self.pronta { Estado::ProntaParaInstalar } else { Estado::Disponivel };
        self.pedido = None;
        self.erro = Some(ERRO_INSTALAR.into());
        aviso("Atualização não instalada", ERRO_INSTALAR)
    }

    /// Desiste do pedido (enquanto ainda não está passando o host nem instalando).
    pub fn cancelar(&mut self) {
        if self.passando_host || self.estado == Estado::Instalando {
            return;
        }
        self.pedido = None;
        if self.estado == Estado::AguardandoMomentoSeguro {
            self.estado = if self.pronta { Estado::ProntaParaInstalar } else { Estado::Disponivel };
        }
    }

    /// Bloqueia Jogar e hospedar (abaixo da versão mínima do grupo).
    pub fn obrigatoria(&self) -> bool {
        self.oferta.as_ref().map(|o| o.obrigatoria).unwrap_or(false)
    }
}

impl Momento {
    fn fase_ocupada_sem_host(&self) -> bool {
        self.fase_ocupada && !self.hospedando
    }
}

fn aviso(titulo: &str, corpo: &str) -> Efeito {
    Efeito::Avisar { titulo: titulo.into(), corpo: corpo.into() }
}

/// `atual` está abaixo de `minima`? Versões inválidas ou vazias nunca bloqueiam.
pub fn abaixo_do_minimo(atual: &str, minima: &str) -> bool {
    match (semver::Version::parse(atual.trim()), semver::Version::parse(minima.trim())) {
        (Ok(a), Ok(m)) => a < m,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oferta(v: &str, obrigatoria: bool) -> Oferta {
        Oferta { versao: v.into(), notas: Some("Novidades".into()), obrigatoria }
    }

    const LIVRE: Momento = Momento { hospedando: false, fase_ocupada: false, replicando: false, trocando: false, sincronizando: false };
    const HOST: Momento = Momento { hospedando: true, fase_ocupada: true, replicando: false, trocando: false, sincronizando: false };

    fn pronta(m: &mut Maquina) {
        assert_eq!(m.verificou(Ok(Some(oferta("2.1.0", false)))), Efeito::Baixar);
        assert_eq!(m.estado, Estado::Baixando);
        assert_eq!(m.baixou(Ok(()), LIVRE), Efeito::Nada);
        assert_eq!(m.estado, Estado::ProntaParaInstalar);
    }

    #[test]
    fn sou_convidado_instala_na_hora() {
        let mut m = Maquina::nova("2.0.3");
        pronta(&mut m);
        assert_eq!(m.pedir(Pedido::Agora, LIVRE), Efeito::Instalar);
        assert_eq!(m.estado, Estado::Instalando);
    }

    #[test]
    fn convidado_replicando_espera_o_momento_seguro() {
        let mut m = Maquina::nova("2.0.3");
        pronta(&mut m);
        let replicando = Momento { replicando: true, ..LIVRE };
        assert_eq!(m.pedir(Pedido::Agora, replicando), Efeito::EsperarMomento);
        assert_eq!(m.estado, Estado::AguardandoMomentoSeguro);
        assert_eq!(m.momento_mudou(Momento { sincronizando: true, ..LIVRE }), Efeito::Nada);
        assert_eq!(m.momento_mudou(LIVRE), Efeito::Instalar);
    }

    #[test]
    fn sou_host_passa_o_host_antes_de_instalar() {
        let mut m = Maquina::nova("2.0.3");
        pronta(&mut m);
        assert_eq!(m.pedir(Pedido::Agora, HOST), Efeito::PassarHost);
        assert_eq!(m.estado, Estado::AguardandoMomentoSeguro);
        assert!(m.passando_host);
        // enquanto passa, nada instala, nem a mudança de status, nem um novo pedido
        assert_eq!(m.momento_mudou(LIVRE), Efeito::Nada);
        assert_eq!(m.pedir(Pedido::Agora, LIVRE), Efeito::Nada);
        assert_eq!(m.host_passado(Ok(FimSeguro::AmigoConfirmou), LIVRE), Efeito::Instalar);
        assert_eq!(m.estado, Estado::Instalando);
    }

    #[test]
    fn host_sozinho_instala_depois_da_nuvem_conferida() {
        let mut m = Maquina::nova("2.0.3");
        pronta(&mut m);
        assert_eq!(m.pedir(Pedido::Agora, HOST), Efeito::PassarHost);
        // a sessão terminou, mas o status ainda mostra a fase de fechamento: espera
        let fechando = Momento { fase_ocupada: true, ..LIVRE };
        assert_eq!(m.host_passado(Ok(FimSeguro::NuvemConferida), fechando), Efeito::EsperarMomento);
        assert_eq!(m.momento_mudou(LIVRE), Efeito::Instalar);
    }

    #[test]
    fn troca_falhou_cancela_e_avisa() {
        let mut m = Maquina::nova("2.0.3");
        pronta(&mut m);
        m.pedir(Pedido::Agora, HOST);
        let e = m.host_passado(Err("ninguém confirmou em 90 s".into()), LIVRE);
        assert!(matches!(e, Efeito::Avisar { .. }));
        assert_eq!(m.estado, Estado::ProntaParaInstalar);
        assert_eq!(m.pedido, None);
        assert_eq!(m.erro.as_deref(), Some(ERRO_TROCA));
        // e não instala sozinho depois
        assert_eq!(m.momento_mudou(LIVRE), Efeito::Nada);
    }

    #[test]
    fn sem_internet_falha_em_silencio() {
        let mut m = Maquina::nova("2.0.3");
        assert_eq!(m.verificou(Err("sem internet".into())), Efeito::Nada);
        assert_eq!(m.estado, Estado::SemAtualizacao);
        assert_eq!(m.erro, None);
        // com uma oferta, cair a internet não apaga o que já se sabe
        m.verificou(Ok(Some(oferta("2.1.0", false))));
        assert_eq!(m.verificou(Err("sem internet".into())), Efeito::Nada);
        assert_eq!(m.oferta.as_ref().unwrap().versao, "2.1.0");
    }

    #[test]
    fn download_caiu_tenta_de_novo_na_proxima_verificacao() {
        let mut m = Maquina::nova("2.0.3");
        m.verificou(Ok(Some(oferta("2.1.0", false))));
        m.pedir(Pedido::Agora, LIVRE);
        let e = m.baixou(Err("conexão caiu".into()), LIVRE);
        assert!(matches!(e, Efeito::Avisar { .. }));
        assert_eq!(m.estado, Estado::Disponivel);
        assert_eq!(m.erro.as_deref(), Some(ERRO_DOWNLOAD));
        assert_eq!(m.verificou(Ok(Some(oferta("2.1.0", false)))), Efeito::Baixar);
        assert_eq!(m.baixou(Ok(()), LIVRE), Efeito::Nada);
        assert_eq!(m.estado, Estado::ProntaParaInstalar);
    }

    #[test]
    fn pedido_antes_do_download_terminar_instala_quando_terminar() {
        let mut m = Maquina::nova("2.0.3");
        m.verificou(Ok(Some(oferta("2.1.0", false))));
        assert_eq!(m.pedir(Pedido::Agora, LIVRE), Efeito::Nada);
        assert_eq!(m.baixou(Ok(()), LIVRE), Efeito::Instalar);
    }

    #[test]
    fn atualizacao_obrigatoria() {
        assert!(abaixo_do_minimo("2.0.3", "2.1.0"));
        assert!(!abaixo_do_minimo("2.1.0", "2.1.0"));
        assert!(!abaixo_do_minimo("2.2.0", "2.1.0"));
        assert!(!abaixo_do_minimo("2.0.3", ""), "sem mínimo (coordenação fora) não bloqueia");
        assert!(!abaixo_do_minimo("2.0.3", "lixo"));
        let mut m = Maquina::nova("2.0.3");
        m.verificou(Ok(Some(oferta("2.1.0", true))));
        assert!(m.obrigatoria());
        m.baixou(Ok(()), LIVRE);
        assert_eq!(m.pedir(Pedido::Agora, LIVRE), Efeito::Instalar);
        let mut n = Maquina::nova("2.0.3");
        n.verificou(Ok(Some(oferta("2.1.0", false))));
        assert!(!n.obrigatoria());
        n.definir_obrigatoria(true);
        assert!(n.obrigatoria());
    }

    #[test]
    fn atualizar_ao_fechar() {
        let mut m = Maquina::nova("2.0.3");
        pronta(&mut m);
        assert_eq!(m.pedir(Pedido::AoFechar, HOST), Efeito::Nada);
        assert_eq!(m.fechando(HOST), Efeito::PassarHost);
        let mut n = Maquina::nova("2.0.3");
        pronta(&mut n);
        assert_eq!(n.fechando(LIVRE), Efeito::Nada, "sem o pedido, fechar só fecha");
    }

    #[test]
    fn nada_novo_volta_ao_inicio() {
        let mut m = Maquina::nova("2.0.3");
        m.verificou(Ok(Some(oferta("2.1.0", false))));
        m.verificou(Ok(None));
        assert_eq!(m.estado, Estado::SemAtualizacao);
        assert_eq!(m.versao_atual, "2.0.3");
    }
}
